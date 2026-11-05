//! Goldens and hit tests use exactly the panels registered in the native flyout controller.
use super::*;
use crate::flyouts::{build, look::Look};
use mypaper_integrations::{docker, media, notifications, obs, t3};
use mypaper_ui::{Point, render};

fn fixtures() -> Vec<(&'static str, Provider, Data)> {
    vec![
        (
            "notifications",
            Provider::Notifications,
            Data::Notifications(notifications::Snapshot {
                cards: vec![
                    notifications::Card {
                        id: 1,
                        app_id: "discord".into(),
                        app: "Discord".into(),
                        title: "Ada Lovelace".into(),
                        body: "The engine notes are ready for review.".into(),
                        at: 134_383_428_600_000_000,
                    },
                    notifications::Card {
                        id: 2,
                        app_id: "grok".into(),
                        app: "Grok Bot".into(),
                        title: "Daily brief is ready".into(),
                        body: "Your routine finished. Open Grok Bot to read it.".into(),
                        at: 134_383_428_000_000_000,
                    },
                ],
                counts: BTreeMap::from([("discord".into(), 1), ("grok".into(), 1)]),
            }),
        ),
        (
            "t3",
            Provider::T3,
            Data::T3(t3::Snapshot {
                threads: vec![
                    t3::Thread {
                        id: "running".into(),
                        project_id: "mypaper".into(),
                        title: "Native app integrations".into(),
                        provider_instance_id: "codex".into(),
                        status: t3::Status::Running,
                        latest_run_id: Some("r1".into()),
                        active_run_id: Some("r1".into()),
                        pending_runtime_request: None,
                        updated_at: "2026-11-05".into(),
                    },
                    t3::Thread {
                        id: "attention".into(),
                        project_id: "mypaper".into(),
                        title: "Review deployment choices".into(),
                        provider_instance_id: "claude".into(),
                        status: t3::Status::Waiting,
                        latest_run_id: Some("r2".into()),
                        active_run_id: Some("r2".into()),
                        pending_runtime_request: Some(t3::Request {
                            id: "q".into(),
                            kind: "user-input".into(),
                        }),
                        updated_at: "2026-11-05".into(),
                    },
                    t3::Thread {
                        id: "finished".into(),
                        project_id: "mypaper".into(),
                        title: "SMTC shared provider".into(),
                        provider_instance_id: "codex".into(),
                        status: t3::Status::Completed,
                        latest_run_id: Some("r3".into()),
                        active_run_id: None,
                        pending_runtime_request: None,
                        updated_at: "2026-11-05".into(),
                    },
                ],
                ..Default::default()
            }),
        ),
        (
            "obs",
            Provider::Obs,
            Data::Obs(obs::Snapshot {
                recording: true,
                streaming: false,
                scene: "Desktop + camera".into(),
                paused: false,
                ..Default::default()
            }),
        ),
        (
            "docker",
            Provider::Docker,
            Data::Docker(docker::Snapshot {
                containers: vec![
                    docker::Container {
                        id: "web".into(),
                        name: "mypaper-web".into(),
                        state: "running".into(),
                        failed: false,
                    },
                    docker::Container {
                        id: "worker".into(),
                        name: "background-worker".into(),
                        state: "exited".into(),
                        failed: true,
                    },
                ],
                ..Default::default()
            }),
        ),
        (
            "media",
            Provider::Media,
            Data::Media(media::Snapshot {
                title: "A Quiet Place".into(),
                artist: "Hania Rani".into(),
                playing: false,
                toggle: true,
                ..Default::default()
            }),
        ),
    ]
}

#[test]
fn integration_panels_match_goldens_receive_clicks_and_have_no_idle_damage() {
    STATE.with_borrow_mut(|s| {
        s.config = Config::default();
        for id in ["running", "attention", "finished"] {
            s.config
                .t3
                .thread_links
                .insert(id.into(), format!("https://example.test/threads/{id}"));
        }
    });
    let mut ui = Ui::new().unwrap();
    let look = Look::default();
    for (name, provider, data) in fixtures() {
        let panel: Box<dyn crate::flyouts::Panel> = if provider == Provider::Notifications {
            Box::new(panels::NotificationCenter::fixture(data))
        } else {
            Box::new(panels::Integration::fixture(provider, data))
        };
        for scale in [1.0, 1.5] {
            let (list, size) = build(
                panel.as_ref(),
                &mut ui.text,
                &look,
                scale,
                1200.0,
                (None, None, None),
                false,
            );
            let (same, _) = build(
                panel.as_ref(),
                &mut ui.text,
                &look,
                scale,
                1200.0,
                (None, None, None),
                false,
            );
            assert!(list.damage(&same).is_none(), "{name} redraws at rest");
            let image = render(&mut ui, &list, size.0, size.1).unwrap();
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(format!("src/integrations/golden/{name}-{scale}.png"));
            mypaper_ui::golden::check(&image, &path).unwrap();
            let (glass, _) = build(
                panel.as_ref(),
                &mut ui.text,
                &look,
                scale,
                1200.0,
                (None, None, None),
                true,
            );
            let image = render(&mut ui, &glass, size.0, size.1).unwrap();
            let radius = look.radius * scale;
            for y in 0..size.1 {
                for x in 0..size.0 {
                    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                    if fx >= radius && fx <= size.0 as f32 - radius
                        || fy >= radius && fy <= size.1 as f32 - radius
                    {
                        assert!(
                            image.pixel(x, y)[3] >= 1,
                            "{name} body lets clicks through at {x},{y}"
                        );
                    }
                }
            }
            for hit in &list.hits {
                let rect = hit.rect;
                assert!(
                    rect.top >= 0.0
                        && rect.left >= 0.0
                        && rect.right <= size.0 as f32
                        && rect.bottom <= size.1 as f32,
                    "{name} control clipped: {}",
                    hit.label
                );
                let center = Point::new(
                    (rect.left + rect.right) / 2.0,
                    (rect.top + rect.bottom) / 2.0,
                );
                if hit.enabled {
                    assert_eq!(
                        list.hit_at(center).map(|h| h.id),
                        Some(hit.id),
                        "{name}: {}",
                        hit.label
                    );
                }
            }
        }
    }
}

#[test]
fn integration_bar_modules_match_their_golden_and_route_every_click() {
    use crate::bars::{look, services::Model, view};
    use mypaper_bars::{BarsConfig, custom::Custom, wake::Waker};
    STATE.with_borrow_mut(|s| {
        s.config.notifications.enabled = true;
        s.config.t3.enabled = true;
        s.config.obs.enabled = true;
        s.config.docker.enabled = true;
        for (_, provider, data) in fixtures() {
            s.fixtures.insert(
                provider,
                Snapshot {
                    availability: Availability::Live,
                    data: Some(data),
                    ..Default::default()
                },
            );
        }
    });
    let mut ui = Ui::new().unwrap();
    let mut config=BarsConfig::parse(r#"{"top":{"left":[],"center":[],"right":["notifications","t3","obs","docker"]}}"#).unwrap();
    config.resolve(Vec::new);
    let tokens = look::Tokens::resolve(
        &config.top.look,
        &mypaper_theme::Appearance::default(),
        &mypaper_bars::accent::read(),
    );
    let (tx, _) = std::sync::mpsc::channel();
    let model = Model {
        foreground: None,
        app_name: String::new(),
        app_icon: None,
        media: None,
        volume: None,
        network: None,
        battery: None,
        stats: None,
        workspaces: mypaper_bars::desktops::Workspaces {
            count: 1,
            current: 0,
        },
        language: String::new(),
        notifications: None,
        tray: vec![],
        clock: Default::default(),
        custom: Custom::new(Waker::new(tx, 0)),
        trash_full: false,
    };
    let mut images = view::Images::default();
    let frame = view::top(
        1600,
        &config,
        &model,
        &tokens,
        1.0,
        &mut ui,
        &mut images,
        None,
    );
    let image = render(&mut ui, &frame.list, frame.size.0, frame.size.1).unwrap();
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/integrations/golden/modules.png");
    mypaper_ui::golden::check(&image, &path).unwrap();
    assert_eq!(frame.list.hits.len(), fixtures().len() - 1);
    for hit in &frame.list.hits {
        let center = Point::new(
            (hit.rect.left + hit.rect.right) / 2.0,
            (hit.rect.top + hit.rect.bottom) / 2.0,
        );
        assert_eq!(frame.target(center).map(|(id, _)| id), Some(hit.id));
        assert!(image.pixel(center.x as u32, center.y as u32)[3] >= 1);
    }
    let same = view::top(
        1600,
        &config,
        &model,
        &tokens,
        1.0,
        &mut ui,
        &mut images,
        None,
    );
    assert!(frame.list.damage(&same.list).is_none());
    let app = mypaper_bars::apps::Item {
        key: "discord".into(),
        name: "Discord".into(),
        path: "discord.exe".into(),
        icon: None,
        pinned: true,
        windows: vec![],
        aumid: Some("discord".into()),
        exe: None,
    };
    let dock = view::dock(
        &config,
        &model,
        &[app],
        &tokens,
        1.0,
        (1600, 100),
        &[],
        &[],
        &mut ui,
        &mut images,
        None,
    );
    let image = render(&mut ui, &dock.list, dock.size.0, dock.size.1).unwrap();
    mypaper_ui::golden::check(
        &image,
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/integrations/golden/dock-notifications.png"),
    )
    .unwrap();
    assert!(
        dock.list
            .ops
            .iter()
            .any(|op| matches!(op,mypaper_ui::Op::Text {text,..} if text.as_ref()=="1")),
        "dock retained-toast badge is missing"
    );
}

#[test]
fn action_panels_match_goldens_and_keep_controls_inside_the_surface() {
    let mut ui = Ui::new().unwrap();
    let look = Look::default();
    let panels: Vec<(&str, Box<dyn crate::flyouts::Panel>)> = vec![
        ("app-actions", Box::new(panels::AppActions::fixture())),
        ("grok-accepted", panels::routine_fixture()),
    ];
    for (name, panel) in panels {
        let (list, size) = build(
            panel.as_ref(),
            &mut ui.text,
            &look,
            1.0,
            900.0,
            (None, None, None),
            false,
        );
        let image = render(&mut ui, &list, size.0, size.1).unwrap();
        mypaper_ui::golden::check(
            &image,
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(format!("src/integrations/golden/{name}.png")),
        )
        .unwrap();
        for hit in list.hits {
            assert!(
                hit.rect.left >= 0.0
                    && hit.rect.top >= 0.0
                    && hit.rect.right <= size.0 as f32
                    && hit.rect.bottom <= size.1 as f32
            );
        }
    }
}
