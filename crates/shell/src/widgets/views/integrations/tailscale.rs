use super::*;
use crate::integrations::widgets::PRIMARY;
use mypaper_integrations::Data;
use mypaper_ui::kit::{Accessory, Lead, Row};

pub(crate) fn view(f: &mut Frame<'_>, m: &Model, rect: Rect, _now_ms: i64) {
    let l = layout(f, rect, "Tailscale", Icon::Globe, true);
    let Some(Data::Tailscale(state)) = &m.integration.snapshot.data else {
        setup(
            f,
            m,
            l,
            "Connect Tailscale",
            "Read your existing Tailscale service. This widget never starts another VPN node.",
            Icon::Globe,
        );
        return;
    };
    let connected = state.state == "Running";
    let online = state.peers.iter().filter(|peer| peer.online).count();
    let peer_summary = if state.peers_known {
        format!("{online} peers online")
    } else {
        "Peer state unavailable".into()
    };
    let (toggle, body) = l.body.take_bottom(f.px(30.0), f.px(7.0));
    f.button(
        PRIMARY,
        toggle,
        if connected { "Disconnect" } else { "Connect" },
        Some(Icon::Power),
        ButtonKind::Gray,
        m.integration.actionable(),
    );
    if l.compact {
        let (status, rest) = body.take_top(f.px(27.0), f.px(1.0));
        f.text_in(
            status,
            if connected { "Connected" } else { &state.state },
            TextRole::Title2,
            if connected {
                f.theme.color.success
            } else {
                f.theme.color.secondary
            },
            Align::Start,
        );
        f.text_in(
            rest,
            &peer_summary,
            TextRole::Callout,
            f.theme.color.secondary,
            Align::Start,
        );
    } else {
        let (summary, peers) = if l.large {
            body.take_top(f.px(97.0), f.px(7.0))
        } else {
            body.take_left(f.px(139.0), f.px(8.0))
        };
        let (status, info) = summary.take_top(f.px(28.0), f.px(2.0));
        f.text_in(
            status,
            if connected { "Connected" } else { &state.state },
            TextRole::Title2,
            if connected {
                f.theme.color.success
            } else {
                f.theme.color.secondary
            },
            Align::Start,
        );
        let address = state.addresses.first().map_or("No address", String::as_str);
        f.label(
            info.left,
            info.top,
            address,
            TextRole::Callout,
            f.theme.color.secondary,
            info.width(),
        );
        if l.large {
            f.label(
                info.left,
                info.top + f.px(23.0),
                &format!(
                    "Exit node · {}",
                    state.exit_node.as_deref().unwrap_or("None")
                ),
                TextRole::Callout,
                f.theme.color.secondary,
                info.width(),
            );
            f.label(
                info.left,
                info.top + f.px(44.0),
                &if state.peers_known {
                    format!("{online} of {} peers online", state.peers.len())
                } else {
                    peer_summary
                },
                TextRole::Caption,
                f.theme.color.tertiary,
                info.width(),
            );
        }
        let row_height = f.px(36.0);
        for (index, peer) in state
            .peers
            .iter()
            .enumerate()
            .take((peers.height() / row_height).floor().max(0.0) as usize)
        {
            f.row(
                &Row {
                    id: 0,
                    lead: Lead::Icon(Icon::Desktop),
                    title: &peer.name,
                    subtitle: None,
                    accessory: Accessory::Text(if peer.online { "Online" } else { "Offline" }),
                    selected: false,
                    enabled: true,
                },
                Rect::new(
                    peers.left,
                    peers.top + index as f32 * row_height,
                    peers.width(),
                    row_height,
                ),
            );
        }
    }
    footer(f, m, l.footer, "Connection live · peers on refresh");
}
