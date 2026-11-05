use super::*;
use crate::integrations::{
    discord::*,
    widgets::{CONNECT, ITEM, PRIMARY, SECONDARY},
};
use crate::widgets::model::Model;
use mypaper_integrations::Data;
use mypaper_ui::{
    Op,
    kit::{Accessory, Lead, Row},
    layout::SizeClass,
};

pub(crate) fn view(f: &mut Frame<'_>, m: &Model, rect: Rect, _now_ms: i64) {
    let inner = rect.inset(Insets::all(f.px(12.0)));
    let (header, content) = inner.take_top(f.px(20.0), f.px(4.0));
    let (refresh, title) = header.take_right(f.px(22.0), f.px(3.0));
    let (settings, title) = title.take_right(f.px(22.0), f.px(3.0));
    f.text_in(
        title,
        "Discord",
        TextRole::Subheadline,
        f.theme.color.secondary,
        Align::Start,
    );
    f.icon_button(
        crate::integrations::widgets::REFRESH,
        refresh,
        Icon::Refresh,
        20.0,
        "Refresh Discord",
        false,
    );
    f.icon_button(
        SETUP,
        settings,
        Icon::Settings,
        20.0,
        "Discord setup",
        false,
    );
    let (footer, body) = content.take_bottom(f.px(14.0), f.px(3.0));
    let state = &m.integration.discord;
    let class = SizeClass::of(rect.width() / f.s, rect.height() / f.s);
    let compact = class == SizeClass::Compact;
    let (tabs, body) = body.take_top(f.px(24.0), f.px(4.0));
    let half = tabs.width() / 2.0;
    f.button(
        MESSAGES,
        Rect::new(tabs.left, tabs.top, half, tabs.height()),
        "Messages",
        None,
        if !state.voice {
            ButtonKind::Tinted
        } else {
            ButtonKind::Plain
        },
        true,
    );
    f.button(
        VOICE,
        Rect::new(tabs.left + half, tabs.top, half, tabs.height()),
        "Call",
        None,
        if state.voice {
            ButtonKind::Tinted
        } else {
            ButtonKind::Plain
        },
        true,
    );
    if state.setup {
        setup_view(f, m, body, compact);
    } else if state.voice {
        call_view(f, m, body, compact);
    } else {
        messages(f, m, body, compact);
    }
    let snapshot = match &m.integration.snapshot.data {
        Some(Data::Discord(s)) => Some(s),
        _ => None,
    };
    let status = state.notice.as_deref().unwrap_or_else(|| {
        if state.voice {
            snapshot.map_or("RPC application and tester access required", |s| {
                if let Some(call) = &s.call {
                    if call.can_control {
                        "Call controls reset when RPC disconnects"
                    } else {
                        "Read-only call · controls require rpc.voice.write"
                    }
                } else {
                    s.voice_status.as_str()
                }
            })
        } else if state.can_send {
            "Sends as bot · Windows toasts are not unread counts"
        } else if !state.channels.is_empty() {
            snapshot.map_or("Add a bot token in Discord setup", |s| {
                s.bot_status.as_str()
            })
        } else if state.notifications_enabled {
            "Windows notifications · refresh for bot history"
        } else {
            "Enable Discord notifications to see recent messages"
        }
    });
    f.text_in(
        footer,
        status,
        TextRole::Caption,
        if state.notice.is_some() {
            f.theme.color.warning
        } else {
            f.theme.color.tertiary
        },
        Align::Start,
    );
}
fn setup_view(f: &mut Frame<'_>, m: &Model, body: Rect, compact: bool) {
    let mut y = body.top;
    let h = f.px(30.0);
    let gap = f.px(7.0);
    for (id, label, icon) in [
        (CONNECT, "Enable recent messages", Icon::Bell),
        (SECONDARY, "Connect current call", Icon::Headphones),
        (BOT, "Add bot to a server", Icon::Add),
    ] {
        if y + h > body.bottom {
            break;
        }
        f.button(
            id,
            Rect::new(body.left, y, body.width(), h),
            label,
            Some(icon),
            ButtonKind::Tinted,
            !m.integration.discord.busy(),
        );
        y += h + gap;
    }
    if !compact && y + f.px(28.0) < body.bottom {
        f.paragraph(body.left,y,body.width(),"Recent messages use Windows notification text. Calls need a registered Discord app and tester/approved RPC access. Bot history needs your bot token and server channel IDs; messages are sent as the bot.",TextRole::Caption,f.theme.color.secondary,4);
    }
}
fn call_view(f: &mut Frame<'_>, m: &Model, body: Rect, compact: bool) {
    let call = match &m.integration.snapshot.data {
        Some(Data::Discord(s)) => s.call.as_ref(),
        _ => None,
    };
    let Some(call) = call else {
        let (button, detail) = body.take_bottom(f.px(30.0), f.px(8.0));
        f.text_in(detail,if matches!(&m.integration.snapshot.data,Some(Data::Discord(s))if s.voice_status=="Connected to Discord RPC"){"No active call"}else{"Connect your current call"},TextRole::Headline,f.theme.color.label,Align::Start);
        f.button(
            SECONDARY,
            button,
            "Connect voice",
            Some(Icon::Headphones),
            ButtonKind::Tinted,
            !m.integration.discord.busy(),
        );
        return;
    };
    let (controls, body) = body.take_bottom(f.px(30.0), f.px(6.0));
    let (heading, rows) = body.take_top(f.px(25.0), f.px(4.0));
    f.text_in(
        heading,
        &call.channel,
        TextRole::Headline,
        f.theme.color.label,
        Align::Start,
    );
    let row = f.px(if compact || rows.height() / f.s < 37.0 {
        22.0
    } else {
        37.0
    });
    for (index, participant) in call
        .participants
        .iter()
        .take((rows.height() / row).floor().max(0.0) as usize)
        .enumerate()
    {
        let at = Rect::new(rows.left, rows.top + index as f32 * row, rows.width(), row);
        let (icon, label) = at.take_left(f.px(24.0), f.px(4.0));
        f.icon(
            icon,
            if participant.deafened {
                Icon::Headphones
            } else if participant.muted {
                Icon::Mute
            } else {
                Icon::Mic
            },
            16.0,
            if participant.speaking {
                f.theme.color.success
            } else {
                f.theme.color.tertiary
            },
        );
        f.text_in(
            label,
            &participant.name,
            TextRole::Callout,
            if participant.speaking {
                f.theme.color.success
            } else {
                f.theme.color.label
            },
            Align::Start,
        );
    }
    let half = (controls.width() - f.px(6.0)) / 2.0;
    f.button(
        MUTE,
        Rect::new(controls.left, controls.top, half, controls.height()),
        if call.muted { "Unmute" } else { "Mute" },
        Some(Icon::Mic),
        ButtonKind::Gray,
        call.can_control,
    );
    f.button(
        DEAFEN,
        Rect::new(
            controls.left + half + f.px(6.0),
            controls.top,
            half,
            controls.height(),
        ),
        if call.deafened { "Undeafen" } else { "Deafen" },
        Some(Icon::Headphones),
        ButtonKind::Gray,
        call.can_control,
    );
}
fn messages(f: &mut Frame<'_>, m: &Model, body: Rect, compact: bool) {
    let state = &m.integration.discord;
    let has_bot = match &m.integration.snapshot.data {
        Some(Data::Discord(s)) => !s.messages.is_empty(),
        _ => false,
    };
    let body = if state.can_send {
        let (send, body) = body.take_bottom(f.px(31.0), f.px(5.0));
        let (button, field) = send.take_right(f.px(48.0), f.px(5.0));
        let hint = if state.reply_to.is_some() {
            "Reply as bot…"
        } else {
            "Send as bot…"
        };
        let inner = f.field_frame(field, hint, m.field.is_none(), m.field.is_some(), None);
        if let Some(field) = &m.field {
            field.set_rect(inner);
            f.push(Op::TextField {
                rect: inner,
                field: field.clone(),
                revision: field.revision(),
            });
        }
        f.hit(
            crate::widgets::views::FIELD,
            field,
            Role::TextInput,
            hint,
            true,
            None,
        );
        f.button(
            SEND,
            button,
            "Send",
            None,
            ButtonKind::Tinted,
            !state.busy(),
        );
        if state.channels.len() > 1 && body.height() > f.px(80.0) {
            let (channels, rest) = body.take_bottom(f.px(23.0), f.px(4.0));
            let width = channels.width() / state.channels.len() as f32;
            for (index, _) in state.channels.iter().enumerate() {
                f.button(
                    CHANNEL + index as u32,
                    Rect::new(
                        channels.left + index as f32 * width,
                        channels.top,
                        width,
                        channels.height(),
                    ),
                    &format!("#{}", index + 1),
                    None,
                    if index == state.channel {
                        ButtonKind::Tinted
                    } else {
                        ButtonKind::Plain
                    },
                    true,
                );
            }
            rest
        } else {
            body
        }
    } else {
        let (open, body) = body.take_bottom(f.px(28.0), f.px(6.0));
        f.button(
            if state.notifications_enabled {
                PRIMARY
            } else {
                CONNECT
            },
            open,
            if state.notifications_enabled {
                "Open Discord"
            } else {
                "Enable recent messages"
            },
            Some(if state.notifications_enabled {
                Icon::External
            } else {
                Icon::Bell
            }),
            ButtonKind::Tinted,
            true,
        );
        body
    };
    let row = f.px(if compact { 37.0 } else { 48.0 });
    let count = (body.height() / row).floor().max(0.0) as usize;
    if has_bot {
        if let Some(Data::Discord(snapshot)) = &m.integration.snapshot.data {
            let selected = state.channels.get(state.channel);
            for (position, (index, message)) in snapshot
                .messages
                .iter()
                .enumerate()
                .filter(|(_, m)| selected.is_none_or(|id| id == &m.channel_id))
                .take(count)
                .enumerate()
            {
                f.row(
                    &Row {
                        id: ITEM + index as u32,
                        lead: Lead::Icon(Icon::Message),
                        title: &message.author,
                        subtitle: Some(if message.content.is_empty() {
                            "Message content unavailable · check bot intent"
                        } else {
                            &message.content
                        }),
                        accessory: Accessory::None,
                        selected: state.reply_to.as_ref() == Some(&message.id),
                        enabled: true,
                    },
                    Rect::new(
                        body.left,
                        body.top + position as f32 * row,
                        body.width(),
                        row,
                    ),
                );
            }
        }
    } else if state.cards.is_empty() {
        f.text_in(
            body,
            if state.notifications_enabled {
                "No retained Discord notifications"
            } else {
                "Connect Discord notifications"
            },
            TextRole::Callout,
            f.theme.color.secondary,
            Align::Start,
        );
    } else {
        for (index, card) in state.cards.iter().take(count).enumerate() {
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
                        "Notification text is hidden"
                    } else {
                        &card.body
                    }),
                    accessory: Accessory::Chevron,
                    selected: false,
                    enabled: true,
                },
                Rect::new(body.left, body.top + index as f32 * row, body.width(), row),
            );
        }
    }
}
