//! The flyouts drawn headless through the same software target the screen gets ([`build`]),
//! with fixed data: golden PNGs (`tests/golden/flyouts`, `MYPAPER_BLESS=1` rewrites them),
//! click coverage of every body and control, placement, and the panels' pure logic.

use mypaper_bars::audio::Volume;
use mypaper_bars::flyouts::index::{App, History};
use mypaper_bars::flyouts::language::Layout;
use mypaper_bars::flyouts::mixer::{Device, MixerState, Session};
use mypaper_bars::flyouts::notifications::{Notification, NotificationList};
use mypaper_bars::flyouts::quick::{PowerMode, RadioInfo, RadioKind};
use mypaper_bars::flyouts::wifi::{Network, Security, WifiState};
use mypaper_bars::system::Battery;
use mypaper_bars::tray_data::TrayItem;
use mypaper_desktop::{Monitor, Rect as Screen};
use mypaper_ui::{Kind as SurfaceKind, Point, Role, Surface, TextField, Ui, render};
use windows::Win32::Foundation::SYSTEMTIME;

use super::calendar::Calendar;
use super::controls;
use super::language::Language;
use super::launcher::{self, Launcher};
use super::look::Look;
use super::mixer::Mixer;
use super::notifications::{self, Center};
use super::power::Power;
use super::quick::Quick;
use super::tray::Overflow;
use super::wifi::{self, Join, Wifi};
use super::{Kind, Panel, build, place};

/// Wednesday 4 November 2026, 09:41.
fn now() -> SYSTEMTIME {
    SYSTEMTIME {
        wYear: 2026,
        wMonth: 11,
        wDayOfWeek: 3,
        wDay: 4,
        wHour: 9,
        wMinute: 41,
        ..Default::default()
    }
}

/// 09:41 that day, ms since the epoch (UTC; only differences matter).
const NOW_MS: u64 = 1_793_785_260_000;

fn note(
    id: u32,
    app: &str,
    aumid: Option<&str>,
    title: &str,
    body: &str,
    minutes_ago: u64,
) -> Notification {
    Notification {
        id,
        app: app.into(),
        aumid: aumid.map(Into::into),
        package_family_name: None,
        icon: None,
        title: title.into(),
        body: body.into(),
        at: NOW_MS - minutes_ago * 60_000,
    }
}

fn notifications() -> NotificationList {
    NotificationList {
        notifications: vec![
            note(
                1,
                "Mail",
                Some("mail"),
                "Ada Lovelace",
                "The engine notes are ready for review.",
                4,
            ),
            note(
                2,
                "Mail",
                Some("mail"),
                "Charles Babbage",
                "Lunch on Thursday?",
                52,
            ),
            note(
                3,
                "Calendar",
                None,
                "Standup in 15 minutes",
                "Room 4 and on Teams",
                120,
            ),
        ],
        error: None,
    }
}

fn panels(ui: &mut Ui, surface: &Surface) -> Vec<(&'static str, Box<dyn Panel>)> {
    let quick = controls::State {
        radios: vec![
            RadioInfo {
                kind: RadioKind::Wifi,
                name: "Wi-Fi".into(),
                on: true,
            },
            RadioInfo {
                kind: RadioKind::Bluetooth,
                name: "Bluetooth".into(),
                on: false,
            },
        ],
        power: Some(PowerMode::Balanced),
        hotspot: None,
        dnd: Some(true),
        night_light: Some(false),
        airplane: Some(false),
    };
    let wifi = WifiState {
        networks: vec![
            Network::new("Home", 92, Security::Wpa2, true, true),
            Network::new("Café Libre", 61, Security::Open, false, false),
            Network::new("Neighbour 5G", 28, Security::Wpa3, false, false),
        ],
        error: None,
    };
    let mixer = MixerState {
        devices: vec![
            Device {
                id: "a".into(),
                name: "Speakers (Realtek Audio)".into(),
                default: true,
            },
            Device {
                id: "b".into(),
                name: "Headphones".into(),
                default: false,
            },
        ],
        sessions: vec![
            Session {
                id: "s".into(),
                name: "System sounds".into(),
                icon: None,
                level: 1.0,
                muted: false,
                system: true,
                active: false,
            },
            Session {
                id: "p".into(),
                name: "Spotify".into(),
                icon: None,
                level: 0.7,
                muted: false,
                system: false,
                active: true,
            },
        ],
        switchable: false,
    };
    let layout = |id: &str, language: &str, layout: &str| Layout {
        id: id.into(),
        language: language.into(),
        layout: layout.into(),
        short: String::new(),
        region: String::new(),
    };
    let tray = |id: &str, name: &str| TrayItem {
        id: id.into(),
        name: name.into(),
        icon: None,
        pinned: false,
    };
    let s = 1.0;
    let field = TextField::new(
        surface.hwnd(),
        Launcher::field_rect(s),
        false,
        Launcher::field_text(s),
        Look::default().theme().color.label,
    )
    .unwrap();
    let app = |name: &str| App {
        id: name.to_lowercase(),
        name: name.into(),
        path: format!("shell:AppsFolder\\{name}"),
        icon: None,
        keywords: vec![],
        packaged: false,
    };
    let apps = [
        "Calculator",
        "Code",
        "Edge",
        "Files",
        "Mail",
        "Photos",
        "Spotify",
        "Terminal",
    ]
    .map(app)
    .to_vec();
    let _ = ui;
    vec![
        (
            "calendar",
            Box::new(Calendar::at(now(), Some(1))) as Box<dyn Panel>,
        ),
        (
            "quick",
            Box::new(Quick::with(
                quick,
                Some(60),
                Some(Volume {
                    level: 0.4,
                    muted: false,
                }),
                Some(Battery {
                    percent: 82,
                    charging: true,
                }),
            )),
        ),
        (
            "wifi",
            Box::new(Wifi::with(wifi, Some(true), Some("Café Libre"))),
        ),
        (
            "mixer",
            Box::new(Mixer::with(
                mixer,
                Some(Volume {
                    level: 0.5,
                    muted: false,
                }),
            )),
        ),
        (
            "notifications",
            Box::new(Center::with(
                notifications(),
                Calendar::at(now(), Some(1)),
                NOW_MS,
            )),
        ),
        (
            "calendar-zones",
            Box::new(Calendar::with_zones(
                now(),
                NOW_MS as i64,
                vec![
                    mypaper_bars::config::Zone {
                        zone: "America/New_York".into(),
                        label: Some("New York".into()),
                    },
                    mypaper_bars::config::Zone {
                        zone: "Asia/Kathmandu".into(),
                        label: None,
                    },
                ],
            )),
        ),
        ("power", Box::new(Power::default())),
        ("power-confirm", Box::new(Power::asking(4))),
        (
            "language",
            Box::new(Language::with(
                vec![
                    layout("4090409", "English (United States)", "US"),
                    layout(
                        "4010401",
                        "العربية (المملكة العربية السعودية)",
                        "Arabic (101)",
                    ),
                    layout("e0010411", "日本語", "Microsoft IME"),
                ],
                Some("4090409".into()),
                0,
            )),
        ),
        (
            "tray",
            Box::new(Overflow::with(vec![
                tray("1", "OneDrive"),
                tray("2", "Steam"),
                tray("3", "Security"),
                tray("4", "Bluetooth devices"),
                tray("5", "Discord"),
            ])),
        ),
        (
            "launcher-win11",
            Box::new(Launcher::win11(field.clone(), apps.clone())),
        ),
        (
            "launcher",
            Box::new(Launcher::with(field, apps, History::default())),
        ),
    ]
}

#[test]
fn every_flyout_matches_its_golden_image_and_takes_clicks_on_its_whole_body() {
    let mut ui = Ui::new().unwrap();
    let surface = Surface::new(
        SurfaceKind::Popup { activate: true },
        "Flyout test",
        Role::Dialog,
    )
    .unwrap();
    let look = Look::default();
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/flyouts");
    for (name, panel) in panels(&mut ui, &surface) {
        // The style's own fill, for the picture.
        let (list, size) = build(
            panel.as_ref(),
            &mut ui.text,
            &look,
            1.0,
            1400.0,
            (None, None, None),
            false,
        );
        let image = render(&mut ui, &list, size.0, size.1).unwrap();
        mypaper_ui::golden::check(&image, &golden.join(format!("{name}.png"))).unwrap();
        // Glass under it: the body is invisible but every pixel of its rounded shape is hit.
        let (list, size) = build(
            panel.as_ref(),
            &mut ui.text,
            &look,
            1.0,
            1400.0,
            (None, None, None),
            true,
        );
        let image = render(&mut ui, &list, size.0, size.1).unwrap();
        let radius = look.radius;
        for y in 0..size.1 {
            for x in 0..size.0 {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let inside_corner = |cx: f32, cy: f32| (fx - cx).hypot(fy - cy) <= radius - 1.5;
                let (w, h) = (size.0 as f32, size.1 as f32);
                let in_shape = (fx >= radius && fx <= w - radius)
                    || (fy >= radius && fy <= h - radius)
                    || inside_corner(radius, radius)
                    || inside_corner(w - radius, radius)
                    || inside_corner(radius, h - radius)
                    || inside_corner(w - radius, h - radius);
                if in_shape {
                    assert!(
                        image.pixel(x, y)[3] >= 1,
                        "{name}: body pixel {x},{y} lets clicks through"
                    );
                }
            }
        }
        // Every control lies on the body and is what a click at its centre lands on.
        for hit in &list.hits {
            let r = hit.rect;
            assert!(
                r.left >= 0.0
                    && r.top >= 0.0
                    && r.right <= size.0 as f32 + 0.5
                    && r.bottom <= size.1 as f32 + 0.5,
                "{name}: {} at {r:?} is outside {size:?}",
                hit.label
            );
            let center = Point::new((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
            assert_eq!(
                list.hit_at(center).map(|h| h.id),
                Some(hit.id),
                "{name}: {}",
                hit.label
            );
            assert!(
                image.pixel(center.x as u32, center.y as u32)[3] >= 1,
                "{name}: {}",
                hit.label
            );
        }
    }
}

#[test]
fn hover_and_focus_redraw_only_the_control_they_touch() {
    let mut ui = Ui::new().unwrap();
    let surface = Surface::new(
        SurfaceKind::Popup { activate: true },
        "Flyout test",
        Role::Dialog,
    )
    .unwrap();
    let look = Look::default();
    let panels = panels(&mut ui, &surface);
    let (_, quick) = &panels[1];
    let (rest, _) = build(
        quick.as_ref(),
        &mut ui.text,
        &look,
        1.0,
        1400.0,
        (None, None, None),
        true,
    );
    let same = build(
        quick.as_ref(),
        &mut ui.text,
        &look,
        1.0,
        1400.0,
        (None, None, None),
        true,
    )
    .0;
    assert_eq!(same.damage(&rest), None, "settled: nothing to redraw");
    let tile = rest
        .hits
        .iter()
        .find(|h| h.label.as_ref() == "Bluetooth")
        .unwrap()
        .clone();
    let (hovered, _) = build(
        quick.as_ref(),
        &mut ui.text,
        &look,
        1.0,
        1400.0,
        (Some(tile.id), None, None),
        true,
    );
    let damage = hovered.damage(&rest).unwrap();
    assert!(
        damage.top >= tile.rect.top - 2.0 && damage.bottom <= tile.rect.bottom + 2.0,
        "{damage:?}"
    );
}

fn monitor() -> Monitor {
    Monitor {
        device: r"\\.\DISPLAY2".into(),
        rect: Screen {
            x: -2560,
            y: 0,
            width: 2560,
            height: 1440,
        },
        work: Screen {
            x: -2560,
            y: 48,
            width: 2560,
            height: 1392,
        },
        dpi: 96,
        primary: false,
    }
}

#[test]
fn flyouts_open_under_their_module_and_stay_in_the_work_area() {
    let m = monitor();
    let clock = mypaper_ui::Rect::new(-1320.0, 8.0, 120.0, 32.0);
    // Centred under the clock, a gap below the bar.
    assert_eq!(
        place(Kind::Calendar, (300, 400), clock, &m, 1.0, true),
        (-1410, 48)
    );
    // Near the right edge it is pushed back inside, keeping the margin.
    let quick = mypaper_ui::Rect::new(-60.0, 8.0, 40.0, 32.0);
    assert_eq!(
        place(Kind::Quick, (340, 500), quick, &m, 1.0, true),
        (-8 - 340, 48)
    );
    // Notifications hug the right edge; the launcher is centred high on the screen.
    assert_eq!(
        place(Kind::Notifications, (380, 900), clock, &m, 1.0, true).0,
        -8 - 380
    );
    assert_eq!(
        place(Kind::Launcher, (560, 400), clock, &m, 1.0, true),
        (-2560 + 1000, 48 + 278)
    );
    // Above a bottom dock.
    let dock = mypaper_ui::Rect::new(-1300.0, 1380.0, 48.0, 48.0);
    let (_, y) = place(Kind::TrayOverflow, (190, 100), dock, &m, 1.0, false);
    assert_eq!(y, 1380 - 8 - 100);
    // Taller than the screen: it starts at the top of the work area.
    assert_eq!(
        place(Kind::Calendar, (300, 3000), clock, &m, 1.0, true).1,
        48
    );
}

#[test]
fn notifications_group_by_app_newest_first_and_say_how_old_they_are() {
    let groups = notifications::by_app(&notifications().notifications);
    let apps: Vec<_> = groups
        .iter()
        .map(|g| (g.app.as_str(), g.items.len()))
        .collect();
    assert_eq!(apps, [("Mail", 2), ("Calendar", 1)]);
    assert_eq!(groups[0].items[0].id, 1, "newest first in the stack");
    assert_eq!(notifications::ago(NOW_MS, NOW_MS), "now");
    assert_eq!(notifications::ago(NOW_MS - 5 * 60_000, NOW_MS), "5 min");
    assert_eq!(notifications::ago(NOW_MS - 3 * 3_600_000, NOW_MS), "3 h");
    assert_eq!(
        notifications::ago(NOW_MS - 30 * 3_600_000, NOW_MS),
        "Yesterday"
    );
}

#[test]
fn the_launcher_ranks_prefixes_then_words_then_letters_and_remembers_runs() {
    assert!(
        launcher::score("cal", "Calculator", &[]) > launcher::score("cal", "Google Calendar", &[])
    );
    assert!(
        launcher::score("code", "Visual Studio Code", &[])
            > launcher::score("code", "Encoder", &[])
    );
    assert!(
        launcher::score("vsc", "Visual Studio Code", &[]).is_some(),
        "letters in order"
    );
    assert_eq!(launcher::score("xyz", "Visual Studio Code", &[]), None);
    let keywords = ["code.exe".to_owned()];
    assert!(
        launcher::score("code", "VS", &keywords).is_some(),
        "the exe's name"
    );
    let apps: Vec<App> = ["Mail", "Maps", "Calculator"]
        .map(|n| App {
            id: n.into(),
            name: n.into(),
            path: n.into(),
            icon: None,
            keywords: vec![],
            packaged: false,
        })
        .to_vec();
    let mut history = History::default();
    history.record("Maps", "Maps", 1);
    let names = |q: &str| {
        launcher::results(&apps, &history, q)
            .iter()
            .map(|a| a.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(""),
        ["Maps", "Calculator", "Mail"],
        "recent first, then by name"
    );
    assert_eq!(
        names("ma"),
        ["Maps", "Mail"],
        "equal scores: the recent one first"
    );
}

#[test]
fn wifi_sends_passwords_to_windows_and_shows_signal_in_four_steps() {
    assert_eq!(
        wifi::join(&Network::new("a", 50, Security::Wpa2, false, false)),
        Join::Prompt
    );
    assert_eq!(
        wifi::join(&Network::new("a", 50, Security::Wpa2, false, true)),
        Join::Direct,
        "saved"
    );
    assert_eq!(
        wifi::join(&Network::new("a", 50, Security::Open, false, false)),
        Join::Direct
    );
    assert_eq!(
        wifi::join(&Network::new("a", 50, Security::Enterprise, false, false)),
        Join::Settings
    );
    assert_eq!(
        wifi::join(&Network::new("a", 50, Security::Wpa3, true, true)),
        Join::Leave
    );
    assert_eq!([0, 30, 60, 100].map(wifi::bars), [1, 2, 3, 4]);
}

#[test]
fn ending_the_session_asks_first_and_locking_does_not() {
    use mypaper_bars::actions::PowerAction::*;
    assert!(Power::confirms(Restart) && Power::confirms(Shutdown) && Power::confirms(Signout));
    assert!(!Power::confirms(Lock) && !Power::confirms(Sleep));
}
