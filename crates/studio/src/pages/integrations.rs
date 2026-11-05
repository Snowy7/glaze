//! Integrations (spec 4.6, after docs/research/integrations.md): a list (Connected, Built in,
//! Available) and the selected one's details. The built-in providers are real and on: media
//! (any player that publishes to Windows, Spotify included), notifications (one listener for
//! Discord and chat apps) and the weather and RSS plugins. The first batch of clients (T3 Code,
//! OBS, Docker, Tailscale, GitHub) is listed as planned: their clients aren't built yet, so
//! Studio says so instead of offering a Connect that does nothing.

use mypaper_ui::controls::{Align, ButtonKind, Font, key};
use mypaper_ui::{Rect, Role};

use crate::actions::{Action, FieldId, Page};
use crate::app::App;
use crate::theme::glyph;
use crate::ui::{B, card, text_field};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    On,
    Planned,
}

pub struct Integration {
    pub name: &'static str,
    pub adds: &'static str,
    pub glyph: char,
    pub status: Status,
    /// What it brings and where it shows.
    pub shows: &'static [(&'static str, &'static str)],
    pub privacy: &'static str,
    /// What uses it, or what's needed before it can connect.
    pub note: &'static str,
}

pub static LIST: &[Integration] = &[
    Integration {
        name: "Media",
        adds: "Any player, Spotify included",
        glyph: glyph::MUSIC,
        status: Status::On,
        shows: &[("Top bar", "The media module: what's playing, play and pause"), ("Widgets", "Now playing and the media pill")],
        privacy: "Reads what Windows' media controls already show. Nothing leaves this PC.",
        note: "Built in: Windows' media sessions (SMTC) cover every player that publishes to them.",
    },
    Integration {
        name: "Notifications",
        adds: "Badges for Discord, Grok and chat apps",
        glyph: glyph::BELL,
        status: Status::On,
        shows: &[("Top bar", "The bell module with a count"), ("Panel", "mypaper's notification center (Win+N, Win+A)")],
        privacy: "Reads Windows' own notifications, with your permission. Nothing leaves this PC.",
        note: "Built in: one listener covers chat apps without a client per app.",
    },
    Integration {
        name: "Weather and RSS",
        adds: "Data for their widgets",
        glyph: glyph::CLOUD,
        status: Status::On,
        shows: &[("Widgets", "Weather, RSS and crypto")],
        privacy: "Weather comes from Open-Meteo (no key); feeds from the addresses you add.",
        note: "Built in: plugins that sleep between fetches.",
    },
    Integration {
        name: "T3 Code",
        adds: "Agent status in the top bar, alerts",
        glyph: glyph::TERMINAL,
        status: Status::Planned,
        shows: &[("Top bar", "Running and waiting agents"), ("Notifications", "Once, when an agent needs you"), ("Panel", "Recent results")],
        privacy: "Read-only, with a token you approve in T3 Code. No transcripts.",
        note: "Planned for the first batch. Connecting needs T3 Code's local token API.",
    },
    Integration {
        name: "OBS Studio",
        adds: "REC and LIVE in the top bar, scenes",
        glyph: glyph::PULSE,
        status: Status::Planned,
        shows: &[("Top bar", "Recording and streaming state")],
        privacy: "Talks to OBS on this PC over its WebSocket, with the password you give it.",
        note: "Planned for the first batch.",
    },
    Integration {
        name: "Docker",
        adds: "Container failures as a badge",
        glyph: glyph::LAYERS,
        status: Status::Planned,
        shows: &[("Top bar", "A badge when a container stops")],
        privacy: "Reads Docker's local engine. Nothing leaves this PC.",
        note: "Planned for the first batch.",
    },
    Integration {
        name: "Tailscale",
        adds: "Connection and exit node",
        glyph: glyph::GLOBE,
        status: Status::Planned,
        shows: &[("Top bar", "Connected or not, and the exit node")],
        privacy: "Reads the local Tailscale client's status.",
        note: "Planned for the first batch.",
    },
    Integration {
        name: "GitHub",
        adds: "Pull requests and checks, on open",
        glyph: glyph::BRANCH,
        status: Status::Planned,
        shows: &[("Panel", "Your pull requests and their checks")],
        privacy: "GitHub's own sign-in page; read-only scopes.",
        note: "Planned for the first batch. Updates only when you open it (no idle wake-ups).",
    },
];

pub fn title(_app: &App) -> (String, String) {
    let on = LIST.iter().filter(|i| i.status == Status::On).count();
    ("Integrations".into(), format!("{on} built in"))
}

pub fn bar(app: &App, b: &mut B, bar: Rect, right: f32) -> f32 {
    let y = bar.top + (bar.height() - b.px(32.0)) / 2.0;
    let field = Rect::new(right - b.px(220.0), y, b.px(220.0), b.px(32.0));
    text_field(app, b, field, FieldId::IntegrationSearch, Some(glyph::SEARCH), "Search integrations", "", "Search integrations");
    field.left - b.px(8.0)
}

pub fn view(app: &App, b: &mut B, body: Rect) {
    let p = *b.p;
    let list_w = b.px(440.0);
    let list = Rect::new(body.left, body.top, list_w, body.height());
    b.vline(list.right, body.top, body.bottom, p.line);
    let needle = app.text(&FieldId::IntegrationSearch).trim().to_lowercase();
    let mut y = list.top + b.px(16.0);
    let x = list.left + b.px(24.0);
    let w = list.width() - b.px(40.0);
    b.group(key("integrations"), |b| {
        for (group, status) in [("Built in", Status::On), ("Available", Status::Planned)] {
            let items: Vec<(usize, &Integration)> = LIST
                .iter()
                .enumerate()
                .filter(|(_, i)| i.status == status)
                .filter(|(_, i)| needle.is_empty() || format!("{} {}", i.name, i.adds).to_lowercase().contains(&needle))
                .collect();
            if items.is_empty() {
                continue;
            }
            let caption = b.font(Font::Caption).scaled(1.0);
            let mut heading = caption.clone();
            heading.weight = 600;
            b.text_at(x + b.px(2.0), y + b.px(4.0), group, &heading, p.ink3);
            y += b.px(28.0);
            for (i, item) in items {
                let r = Rect::new(x - b.px(4.0), y, w + b.px(8.0), b.px(52.0));
                let k = key(("integration", i));
                let on = app.nav.integration == i;
                if on {
                    b.fill(r, b.px(8.0), p.selected);
                } else if b.hovered(k) {
                    b.fill(r, b.px(8.0), p.hover);
                }
                let tile = Rect::new(r.left + b.px(10.0), r.top + b.px(10.0), b.px(32.0), b.px(32.0));
                b.fill(tile, b.px(8.0), p.hover);
                b.icon(tile, item.glyph, 15.0, p.ink2);
                let strong = b.font(Font::Strong);
                let small = b.font(Font::Caption);
                b.text_at(tile.right + b.px(12.0), r.top + b.px(8.0), item.name, &strong, p.ink1);
                b.text_in(Rect::new(tile.right + b.px(12.0), r.top + b.px(27.0), r.width() - b.px(160.0), b.px(16.0)), item.adds, &small, p.ink3, Align::Start);
                let status = match item.status {
                    Status::On => "On",
                    Status::Planned => "Planned",
                };
                b.text_in(Rect::new(r.right - b.px(110.0), r.top, b.px(100.0), r.height()), status, &small, if item.status == Status::On { p.ink2 } else { p.ink4 }, Align::End);
                b.hit_checked(k, r, Role::ListItem, &format!("{}: {}", item.name, status), Some(on), Action::SelectIntegration(i), mypaper_ui::controls::Behavior::Click);
                b.focus_ring(k, r, b.px(8.0));
                y += b.px(54.0);
            }
            y += b.px(10.0);
        }
    });
    details(app, b, Rect::new(list.right + b.px(24.0), body.top + b.px(20.0), body.right - list.right - b.px(48.0), body.height() - b.px(40.0)));
}

fn details(app: &App, b: &mut B, area: Rect) {
    let p = *b.p;
    let Some(item) = LIST.get(app.nav.integration) else { return };
    let rows = item.shows.len() as f32;
    let h = b.px(76.0) + b.px(40.0) + rows * b.px(46.0) + b.px(110.0) + b.px(52.0);
    let r = Rect::new(area.left, area.top, area.width(), h.min(area.height()));
    b.fill(r, b.px(12.0), p.chrome);
    b.stroke(r, b.px(12.0), 1.0, p.edge);
    let x = r.left + b.px(16.0);
    let w = r.width() - b.px(32.0);
    let tile = Rect::new(x, r.top + b.px(16.0), b.px(44.0), b.px(44.0));
    b.fill(tile, b.px(11.0), p.hover);
    b.icon(tile, item.glyph, 20.0, p.ink1);
    let t2 = b.font(Font::Title2);
    let small = b.font(Font::Caption);
    b.text_at(tile.right + b.px(14.0), tile.top + b.px(2.0), item.name, &t2, p.ink1);
    let state = match item.status {
        Status::On => "On · built in",
        Status::Planned => "Not available yet",
    };
    b.dot(mypaper_ui::Point::new(tile.right + b.px(18.0), tile.top + b.px(32.0)), item.status == Status::On);
    b.text_at(tile.right + b.px(28.0), tile.top + b.px(24.0), state, &small, p.ink3);
    let (label, enabled) = match item.status {
        Status::On => ("Built in", false),
        Status::Planned => ("Connect", false),
    };
    let bw = b.button_width(label, false, false);
    b.button(key("integration-connect"), Rect::new(r.right - b.px(16.0) - bw, tile.top + b.px(6.0), bw, b.px(32.0)), label, None, ButtonKind::Secondary, enabled, Action::None);
    let mut y = r.top + b.px(76.0);
    b.hline(r.left, r.right, y, p.line);
    y += b.px(14.0);
    let heading = b.font(Font::Heading);
    b.text_at(x, y, "Shows in", &heading, p.ink1);
    y += b.px(26.0);
    let body = b.font(Font::Body);
    for (place, what) in item.shows {
        b.text_at(x, y, place, &body, p.ink1);
        b.text_in(Rect::new(x, y + b.px(20.0), w, b.px(16.0)), what, &small, p.ink3, Align::Start);
        y += b.px(46.0);
    }
    b.hline(r.left, r.right, y, p.line);
    y += b.px(14.0);
    b.text_at(x, y, "About it", &heading, p.ink1);
    y += b.px(26.0);
    y += b.paragraph(x, y, w, item.note, &small, b.px(16.0), p.ink2);
    let link = match item.status {
        Status::On if item.name == "Weather and RSS" => Some(("Their widgets", Action::Nav(Page::Widgets))),
        Status::On => Some(("Their bar modules", Action::Nav(Page::Bars))),
        Status::Planned => None,
    };
    if let Some((label, action)) = link {
        y += b.px(8.0);
        let lw = b.button_width(label, true, true);
        b.button(key("integration-link"), Rect::new(x, y, lw, b.px(28.0)), label, Some(glyph::CHEVRON_RIGHT), ButtonKind::Secondary, true, action);
    }
    let foot = Rect::new(r.left, r.bottom - b.px(48.0), r.width(), b.px(48.0));
    b.hline(r.left, r.right, foot.top, p.line);
    b.icon(Rect::new(x, foot.top, b.px(16.0), foot.height()), glyph::INFO, 13.0, p.ink3);
    b.text_in(Rect::new(x + b.px(26.0), foot.top, w - b.px(26.0), foot.height()), item.privacy, &small, p.ink3, Align::Start);
    let _ = card;
}
