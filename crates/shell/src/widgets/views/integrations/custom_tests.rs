//! Render the actual dispatch at every dense size and inspect representative size sheets.
use super::*;
use crate::widgets::{time::Stamp, views};
use mypaper_integrations::{Data, Snapshot, calendar, discord, notifications};
use mypaper_ui::{DisplayList, Op, Tokens, Ui, golden, render};
use mypaper_widgets::{
    custom::{Definition, Template},
    layout::Placed,
    storage::Storage,
};
use serde_json::{Map, json};
use std::{path::PathBuf, sync::Arc, time::Instant};
const NOW: i64 = 1_793_875_200_000;
fn fixture(id: &str) -> Model {
    let placed = Placed {
        instance: format!("{id}-personal"),
        widget: id.into(),
        cell: [0, 0],
        size: [6, 4],
        settings: Map::new(),
        appearance: Map::new(),
        own: Map::new(),
    };
    let mut m = Model::new(
        placed,
        &mut Storage::new(std::env::temp_dir().join("mypaper-personal-sheets")),
        Stamp::at(NOW),
    )
    .unwrap();
    m.visible = true;
    if id == "calendar" {
        m.events = vec![
            json!({"date":"2026-11-05","title":"Design review","time":"10:30","calendarColor":[10,132,255],"source":"calendar","readOnly":true}),
            json!({"date":"2026-11-05","title":"Walk outside","time":"16:00","calendarColor":[48,209,88],"source":"calendar","readOnly":true}),
            json!({"date":"2026-11-12","title":"Launch day","color":4}),
        ];
        m.integration.snapshot = Snapshot {
            availability: Availability::Cached,
            data: Some(Data::Calendar(calendar::Snapshot {
                sources: std::collections::BTreeMap::from([
                    ("Windows".into(), "2 events".into()),
                    ("Google".into(), "4 events · snapshot".into()),
                    ("Microsoft".into(), "Not connected".into()),
                ]),
                ..Default::default()
            })),
            ..Default::default()
        };
    } else if id == "discord" {
        m.integration.discord.notifications_enabled = true;
        m.integration.discord.cards = vec![
            notifications::Card {
                id: 1,
                app_id: "com.squirrel.Discord.Discord".into(),
                app: "Discord".into(),
                title: "Ada · Design room".into(),
                body: "The native widget layouts are ready.".into(),
                at: 0,
            },
            notifications::Card {
                id: 2,
                app_id: "com.squirrel.Discord.Discord".into(),
                app: "Discord".into(),
                title: "Lin · Desktop".into(),
                body: "I left notes on the calendar.".into(),
                at: 0,
            },
        ];
        m.integration.snapshot = Snapshot {
            availability: Availability::Live,
            data: Some(Data::Discord(discord::Snapshot {
                voice_status: "Connected to Discord RPC".into(),
                call: Some(discord::Call {
                    channel_id: "42".into(),
                    channel: "Design room".into(),
                    participants: vec![
                        discord::Participant {
                            id: "1".into(),
                            name: "Alex".into(),
                            speaking: true,
                            ..Default::default()
                        },
                        discord::Participant {
                            id: "2".into(),
                            name: "Ada".into(),
                            muted: true,
                            ..Default::default()
                        },
                        discord::Participant {
                            id: "3".into(),
                            name: "Lin".into(),
                            ..Default::default()
                        },
                    ],
                    can_control: true,
                    ..Default::default()
                }),
                ..Default::default()
            })),
            ..Default::default()
        };
    } else {
        m.integration.custom.definition =
            Some(Definition::parse(mypaper_widgets::custom::EXAMPLES[0].1).unwrap());
        m.integration.custom.value =
            json!("Build complex things as simply as possible.\n\nOne useful thing at a time.");
    }
    m
}
fn draw(ui: &mut Ui, m: &Model, pixels: (u32, u32), s: f32) -> DisplayList {
    views::view(
        m,
        &Tokens::default(),
        views::Frame {
            size: pixels,
            scale: s,
            now: Stamp::at(NOW),
            instant: Instant::now(),
            editing: false,
        },
        &mut ui.text,
    )
}
fn add(ui: &mut Ui, sheet: &mut DisplayList, m: &Model, pixels: (u32, u32), x: f32, y: f32) {
    let list = draw(ui, m, pixels, 1.0);
    let image = Arc::new(render(ui, &list, pixels.0, pixels.1).unwrap());
    sheet.ops.push(Op::Image {
        rect: Rect::new(x, y, pixels.0 as f32, pixels.1 as f32),
        image,
        opacity: 1.0,
        source: None,
    });
}
#[test]
fn personal_widgets_dense_sizes_and_size_sheets() {
    let mut ui = Ui::new().unwrap();
    for id in ["discord", "calendar", "custom-fixture"] {
        let min = if id == "custom-fixture" {
            [2, 2]
        } else if id == "calendar" {
            [4, 2]
        } else {
            [4, 4]
        };
        for w in min[0]..=12 {
            for h in min[1]..=12 {
                let m = fixture(id);
                let pixels = (w * 45, h * 45);
                let list = draw(&mut ui, &m, pixels, 1.0);
                let same = draw(&mut ui, &m, pixels, 1.0);
                assert!(
                    list.damage(&same).is_none(),
                    "{id} redraws without a change"
                );
                for hit in &list.hits {
                    assert!(
                        hit.rect.left >= 0.0
                            && hit.rect.top >= 0.0
                            && hit.rect.right <= pixels.0 as f32
                            && hit.rect.bottom <= pixels.1 as f32,
                        "{id} {w}×{h}: {} {:?}",
                        hit.label,
                        hit.rect
                    );
                }
                if id == "discord" {
                    let mut call = fixture(id);
                    call.integration.discord.voice = true;
                    let list = draw(&mut ui, &call, pixels, 1.0);
                    for hit in &list.hits {
                        assert!(
                            hit.rect.top >= 0.0 && hit.rect.bottom <= pixels.1 as f32,
                            "{id} call {w}×{h}: {} {:?}",
                            hit.label,
                            hit.rect
                        );
                    }
                }
                if id == "custom-fixture" {
                    for template in [
                        Template::Stat,
                        Template::List,
                        Template::Gauge,
                        Template::Image,
                    ] {
                        let mut m = fixture(id);
                        m.integration.custom.definition.as_mut().unwrap().layout = template;
                        m.integration.custom.value = json!(68);
                        m.integration.custom.image = Some(Arc::new(mypaper_ui::Image {
                            width: 2,
                            height: 2,
                            pixels: vec![80, 120, 220, 255].repeat(4),
                        }));
                        let scale = 1.5;
                        let pixels = (
                            (w as f32 * 45.0 * scale).round() as u32,
                            (h as f32 * 45.0 * scale).round() as u32,
                        );
                        let list = draw(&mut ui, &m, pixels, scale);
                        for hit in &list.hits {
                            assert!(
                                hit.rect.left >= 0.0
                                    && hit.rect.top >= 0.0
                                    && hit.rect.right <= pixels.0 as f32
                                    && hit.rect.bottom <= pixels.1 as f32,
                                "{template:?} {w}×{h} at 150%: {} {:?}",
                                hit.label,
                                hit.rect
                            );
                        }
                    }
                }
                if id == "calendar" {
                    let mut m = fixture(id);
                    m.integration.calendar.show_sources = true;
                    let list = draw(&mut ui, &m, pixels, 1.0);
                    for hit in &list.hits {
                        assert!(
                            hit.rect.top >= 0.0 && hit.rect.bottom <= pixels.1 as f32,
                            "Calendar sources {w}×{h}: {} {:?}",
                            hit.label,
                            hit.rect
                        );
                    }
                }
            }
        }
        let mut sheet = DisplayList::default();
        sheet.ops.push(Op::Fill {
            rect: Rect::new(0.0, 0.0, 1100.0, 800.0),
            radius: 0.0,
            color: Rgba::rgb8([22, 29, 40], 1.0),
        });
        for (pixels, x) in [((180, 180), 20.0), ((360, 225), 220.0), ((450, 360), 600.0)] {
            let mut m = fixture(id);
            if id == "discord" && pixels.0 == 450 {
                m.integration.discord.voice = true;
            }
            add(&mut ui, &mut sheet, &m, pixels, x, 20.0);
        }
        if id == "calendar" {
            let mut m = fixture(id);
            m.integration.calendar.show_sources = true;
            add(&mut ui, &mut sheet, &m, (360, 300), 20.0, 410.0);
            m.integration.calendar.show_sources = false;
            m.placed.settings.insert("variant".into(), json!("agenda"));
            add(&mut ui, &mut sheet, &m, (360, 300), 410.0, 410.0);
        } else if id == "discord" {
            let mut m = fixture(id);
            m.integration.discord.setup = true;
            add(&mut ui, &mut sheet, &m, (360, 300), 20.0, 410.0);
            m.integration.discord.setup = false;
            m.integration.discord.voice = true;
            add(&mut ui, &mut sheet, &m, (180, 180), 410.0, 410.0);
            m.integration.discord.voice = false;
            m.integration.discord.can_send = true;
            m.integration.discord.channels = vec!["42".into()];
            if let Some(Data::Discord(s)) = &mut m.integration.snapshot.data {
                s.messages = vec![discord::Message {
                    id: "100".into(),
                    channel_id: "42".into(),
                    author: "Ada".into(),
                    content: "Ready for the next iteration.".into(),
                    timestamp: String::new(),
                }];
            }
            add(&mut ui, &mut sheet, &m, (360, 300), 700.0, 410.0);
        } else {
            for (template, x) in [
                (Template::Stat, 20.0),
                (Template::List, 290.0),
                (Template::Gauge, 560.0),
            ] {
                let mut m = fixture(id);
                m.integration.custom.definition.as_mut().unwrap().layout = template;
                m.integration.custom.value = match template {
                    Template::Stat => json!("1.91.0"),
                    Template::List => json!([
                        "A Philosophy of Software Design",
                        "The Pragmatic Programmer",
                        "Designing Data-Intensive Applications"
                    ]),
                    _ => json!(68),
                };
                m.integration.custom.definition.as_mut().unwrap().title = match template {
                    Template::Stat => "Rust release",
                    Template::List => "Reading list",
                    _ => "C: drive used",
                }
                .into();
                if template == Template::Gauge {
                    m.integration.custom.definition.as_mut().unwrap().suffix = "%".into();
                }
                add(&mut ui, &mut sheet, &m, (250, 250), x, 410.0);
            }
        }
        let image = render(&mut ui, &sheet, 1100, 800).unwrap();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            "src/widgets/views/integrations/golden-personal/{id}.png"
        ));
        golden::check(&image, &path).unwrap();
    }
}

#[test]
fn imported_calendar_occurrences_leave_local_events_and_edit_limits_alone() {
    let mut m = fixture("calendar");
    m.events = vec![json!({"id":"local","date":"2026-11-05","title":"Local reminder"})];
    if let Some(Data::Calendar(snapshot)) = &mut m.integration.snapshot.data {
        snapshot.events = vec![calendar::Event {
            id: "remote".into(),
            calendar_id: "google:work".into(),
            calendar: "Work".into(),
            title: "Work appointment".into(),
            start_ms: NOW,
            end_ms: NOW + 3600000,
            all_day: false,
            color: [10, 132, 255],
            start_date: None,
            end_date: None,
        }];
    }
    crate::integrations::calendar::sync(&mut m);
    assert_eq!(m.events.len(), 1);
    assert_eq!(m.events[0]["id"], "local");
    assert_eq!(m.integration.calendar.events.len(), 1);
    assert_eq!(m.integration.calendar.events[0]["readOnly"], true);
    assert_eq!(crate::integrations::calendar::events(&m).count(), 2);
    let mut ui = Ui::new().unwrap();
    m.placed.settings.insert("variant".into(), json!("agenda"));
    let list = draw(&mut ui, &m, (450, 400), 1.0);
    assert_eq!(
        list.hits
            .iter()
            .filter(|hit| hit.label.as_ref() == "Edit event")
            .count(),
        1
    );
}
