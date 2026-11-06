mod bars;
mod connections;
pub use bars::{insert_module, move_module};
pub use connections::{connect, connection_module, disconnect, hide_connection_widget};
mod look;
mod settings;
pub use settings::add_menu_item;
mod wallpapers;
mod widgets;
pub use widgets::{can_add_widget, pick_asset};

use mypaper_ui::controls::{Align, Behavior, key};
use mypaper_ui::{Rect, Role};
use mypaper_widgets::layout::{Instance, Layout};
use serde_json::Value;

use super::controls::{button, group, height, notice, row, text, title};
use super::{Command, Page};
use crate::actions::{Action, Choices, Edit, FieldId};
use crate::app::App;
use crate::docs::DocId;
use crate::theme::glyph;
use crate::ui::{B, Col};

pub fn open(page: Page) -> Action {
    Action::Panel(Command::Open(page))
}

pub fn view(app: &App, b: &mut B, c: &mut Col, page: Page) {
    match page {
        Page::Widget => widgets::widget(app, b, c),
        Page::AddWidget => widgets::add(app, b, c),
        Page::Look => look::view(app, b, c),
        Page::Windows => look::windows(app, b, c),
        Page::Wallpapers => wallpapers::list(app, b, c),
        Page::Wallpaper => wallpapers::details(app, b, c),
        Page::AddWallpaper => wallpapers::add(app, b, c),
        Page::Connections => connections::list(app, b, c),
        Page::Connection => connections::details(app, b, c),
        Page::General => settings::general(app, b, c),
        Page::Keyboard => settings::keyboard(b, c),
        Page::Troubleshoot => settings::troubleshoot(app, b, c),
        Page::About => settings::about(app, b, c),
        Page::DesktopMenu => settings::desktop_menu(app, b, c),
        Page::History => history(app, b, c),
        Page::Search => search(app, b, c),
        Page::Top | Page::Dock => bars::bar(app, b, c, page),
        Page::Module => bars::module(app, b, c),
        Page::AddModule => bars::add(app, b, c),
        Page::Panels => bars::panels(b, c),
        Page::BarPanel => bars::panel(app, b, c),
        Page::FirstRun => super::first_run::view(app, b, c),
        _ => {}
    }
}

/// Position is relative to the display's actual grid, including the widget's size.
pub fn position(app: &App, layout: &Layout, instance: &Instance) -> String {
    let Some(monitor) = app.display() else {
        return "on this display".into();
    };
    let grid = crate::stage::grid_for(layout, monitor);
    let x = (instance.cell[0] as f32 + instance.size[0] as f32 / 2.0) / grid.cols.max(1) as f32;
    let y = (instance.cell[1] as f32 + instance.size[1] as f32 / 2.0) / grid.rows.max(1) as f32;
    let area = match (
        (x * 3.0).floor().min(2.0) as u8,
        (y * 3.0).floor().min(2.0) as u8,
    ) {
        (0, 0) => "top left",
        (1, 0) => "top",
        (2, 0) => "top right",
        (0, 1) => "left",
        (1, 1) => "center",
        (2, 1) => "right",
        (0, _) => "bottom left",
        (1, _) => "bottom",
        _ => "bottom right",
    };
    format!(
        "{area}, column {}, row {}",
        instance.cell[0] + 1,
        instance.cell[1] + 1
    )
}

/// The full tile is selected; its thumbnail uses the shell's offscreen bar views.
pub fn design_row(
    app: &App,
    b: &mut B,
    c: &mut Col,
    id: &str,
    name: &str,
    on: bool,
    action: Action,
) {
    let r = Rect::new(c.x, c.y + b.px(3.0), c.w, b.px(80.0));
    let k = key(("design", c.view, id));
    if on {
        b.fill(r, b.px(8.0), b.p.fill);
        b.stroke(r, b.px(8.0), b.px(1.0), b.p.ink2);
    } else {
        b.wash(k, r, b.px(8.0));
    }
    let pic = Rect::new(
        r.left + b.px(6.0),
        r.top + b.px(6.0),
        b.px(130.0),
        r.height() - b.px(12.0),
    );
    crate::panel::specimens::backdrop(app, b, pic);
    let (raw, only) = match &action {
        Action::Set(edit, value) => {
            let mut raw = app.docs.doc(DocId::Bars).raw.clone();
            crate::docs::apply_op(
                &mut raw,
                &crate::docs::Op {
                    doc: DocId::Bars,
                    path: edit.path.to_string(),
                    value: Some(value.clone()),
                },
            );
            (
                raw,
                Some(if edit.path.starts_with("/top") {
                    crate::actions::Bar::Top
                } else {
                    crate::actions::Bar::Dock
                }),
            )
        }
        _ => (serde_json::json!({"version":1,"preset":id}), None),
    };
    crate::panel::specimens::bars(app, b, pic, &raw, only);
    text(
        b,
        Rect::new(
            pic.right + b.px(12.0),
            r.top,
            r.right - pic.right - b.px(36.0),
            r.height(),
        ),
        name,
        13.0,
        500,
        b.p.ink1,
        Align::Start,
    );
    if on {
        b.icon(
            Rect::new(
                r.right - b.px(26.0),
                r.top + b.px(28.0),
                b.px(20.0),
                b.px(20.0),
            ),
            glyph::CHECK,
            15.0,
            b.p.ink1,
        );
    }
    b.hit_checked(
        k,
        r,
        Role::RadioButton,
        name,
        Some(on),
        action,
        Behavior::Click,
    );
    b.focus_ring(k, r, b.px(8.0));
    c.y = r.bottom + b.px(3.0);
}

pub fn field(
    app: &App,
    b: &mut B,
    c: &mut Col,
    id: FieldId,
    label: &str,
    placeholder: &str,
    value: &str,
) {
    let h = height(b, 22.0);
    text(
        b,
        Rect::new(c.x, c.y, c.w, h),
        label,
        13.0,
        400,
        b.p.ink1,
        Align::Start,
    );
    let r = Rect::new(c.x, c.y + h, c.w, height(b, 32.0));
    crate::ui::text_field(app, b, r, id, None, placeholder, value, label);
    b.stroke(r, b.px(6.0), b.px(1.0), b.p.edge);
    c.y = r.bottom + b.px(10.0);
}

pub fn select(
    b: &mut B,
    c: &mut Col,
    label: &str,
    value: Value,
    edit: Edit,
    mut options: Vec<(Value, String)>,
) {
    options.sort_by_key(|(_, label)| label != "As is");
    let shown = options
        .iter()
        .find(|(v, _)| v == &value)
        .map_or_else(|| "Custom".to_owned(), |(_, l)| l.clone());
    row(
        b,
        c,
        None,
        label,
        &shown,
        "",
        None,
        Action::Choose(Choices {
            edit,
            options,
            current: Some(value),
        }),
    );
}

pub fn toggle(b: &mut B, c: &mut Col, label: &str, caption: &str, on: bool, action: Action) {
    let h = height(b, if caption.is_empty() { 40.0 } else { 56.0 });
    text(
        b,
        Rect::new(
            c.x,
            c.y,
            c.w - b.px(50.0),
            height(b, if caption.is_empty() { 40.0 } else { 28.0 }),
        ),
        label,
        13.0,
        400,
        b.p.ink1,
        Align::Start,
    );
    if !caption.is_empty() {
        text(
            b,
            Rect::new(
                c.x,
                c.y + height(b, 28.0),
                c.w - b.px(50.0),
                height(b, 18.0),
            ),
            caption,
            12.0,
            400,
            b.p.ink3,
            Align::Start,
        );
    }
    let point = mypaper_ui::Point::new(c.right() - b.px(36.0), c.y + (h - b.px(20.0)) / 2.0);
    b.switch(
        key(("panel-toggle", c.view, label)),
        point,
        on,
        label,
        action,
    );
    c.y += h;
}

pub fn action_button(b: &mut B, c: &mut Col, label: &str, action: Action) {
    let r = Rect::new(c.x, c.y + b.px(6.0), c.w, height(b, 30.0));
    button(b, r, label, None, false, true, action);
    c.y = r.bottom + b.px(8.0);
}

pub fn footer_height(app: &App) -> f32 {
    match app.panel.as_ref().map(|s| s.page) {
        Some(Page::AddWidget | Page::AddWallpaper | Page::AddModule | Page::FirstRun) => 64.0,
        Some(Page::History)
            if app
                .panel
                .as_ref()
                .unwrap()
                .history
                .borrow()
                .selected
                .is_some() =>
        {
            64.0
        }
        _ => 0.0,
    }
}

pub fn footer(app: &App, b: &mut B, r: Rect) {
    if r.height() <= 0.0 {
        return;
    }
    b.fill(r, 0.0, b.p.chrome);
    b.hline(r.left, r.right, r.top, b.p.line);
    let state = app.panel.as_ref().unwrap();
    let (secondary, cancel, primary, commit, enabled) = match state.page {
        Page::FirstRun => (
            "Skip",
            Action::FirstRunSkip,
            if matches!(
                app.overlay,
                Some(crate::app::Overlay::FirstRun { step: 2, .. })
            ) {
                "Open Studio"
            } else {
                "Continue"
            },
            Action::FirstRunNext,
            true,
        ),
        Page::AddWidget => (
            "Cancel",
            open(Page::Widgets),
            "Add",
            Action::Panel(Command::AddWidget),
            state
                .add_widget
                .as_deref()
                .is_some_and(|id| can_add_widget(app, id)),
        ),
        Page::AddWallpaper => (
            "Cancel",
            open(Page::Wallpapers),
            "Add and use",
            Action::AddCommit(true),
            wallpapers::can_add(app),
        ),
        Page::AddModule => (
            "Cancel",
            open(if app.nav.bar == Some(crate::actions::Bar::Dock) {
                Page::Dock
            } else {
                Page::Top
            }),
            "Add",
            Action::Panel(Command::AddModule),
            state
                .add_module
                .as_deref()
                .is_some_and(|id| bars::can_insert_module(app, id)),
        ),
        Page::History => (
            "Cancel",
            Action::Panel(Command::CancelRestore),
            "Restore this desktop",
            Action::Panel(Command::Restore),
            true,
        ),
        _ => return,
    };
    if state.page == Page::AddWallpaper {
        let widths = [
            70.0,
            70.0,
            (r.width() / b.s - 32.0 - 16.0 - 140.0).max(100.0),
        ];
        let mut x = b.px(16.0);
        for ((label, action, primary), width) in [
            ("Cancel", open(Page::Wallpapers), false),
            ("Add", Action::AddCommit(false), false),
            ("Add and use", Action::AddCommit(true), true),
        ]
        .into_iter()
        .zip(widths)
        {
            button(
                b,
                Rect::new(x, r.top + b.px(16.0), b.px(width), b.px(32.0)),
                label,
                None,
                primary,
                label == "Cancel" || enabled,
                action,
            );
            x += b.px(width + 8.0);
        }
        return;
    }
    let gap = b.px(8.0);
    let width = (r.width() - b.px(32.0) - gap) / 2.0;
    button(
        b,
        Rect::new(b.px(16.0), r.top + b.px(16.0), width, b.px(32.0)),
        secondary,
        None,
        false,
        true,
        cancel,
    );
    button(
        b,
        Rect::new(
            b.px(16.0) + width + gap,
            r.top + b.px(16.0),
            width,
            b.px(32.0),
        ),
        primary,
        None,
        true,
        enabled,
        commit,
    );
}

fn history(app: &App, b: &mut B, c: &mut Col) {
    title(b, c, "History", "Your desktop after each change");
    notice(
        b,
        c,
        "Select a desktop to inspect it, then restore. Later desktops stay here. Settings and Connections are separate.",
    );
    let history = app.panel.as_ref().unwrap().history.borrow();
    if let Some(error) = &history.error {
        let mut actions = vec![];
        if let Some(path) = crate::panel::history::path(app) {
            actions.push(("Show file", Action::Reveal(path)));
        }
        if error != crate::docs::NEWER {
            actions.push(("Retry", Action::Panel(Command::RetryHistory)));
        }
        crate::panel::controls::feedback(b, c, error, actions);
    }
    group(b, c, "Desktop changes", |b, c| {
        for (i, entry) in history.entries.iter().enumerate().rev() {
            row(
                b,
                c,
                Some(glyph::CLOCK),
                if entry.summary.is_empty() {
                    &entry.title
                } else {
                    &entry.summary
                },
                if i + 1 == history.entries.len() {
                    "Now"
                } else {
                    ""
                },
                &format!(
                    "{} · {}",
                    super::history::time_label(entry.at),
                    entry.detail
                ),
                Some(history.selected == Some(i)),
                Action::Panel(Command::History(i)),
            );
        }
    });
    if let Some(entry) = history.selected.and_then(|i| history.entries.get(i)) {
        group(b, c, "Selected desktop", |b, c| {
            let r = Rect::new(c.x, c.y + b.px(6.0), c.w, c.w * 9.0 / 16.0);
            b.fill(r, b.px(8.0), b.p.fill);
            if let Some(wallpaper) = entry.desktop.wallpaper().and_then(|id| {
                app.library
                    .items
                    .iter()
                    .find(|w| w.apply_as == id || w.id == id)
            }) && let Some(path) = wallpaper.preview.as_ref()
                && let Some(image) = app
                    .images
                    .borrow_mut()
                    .get(path, (r.width() as u32, r.height() as u32))
            {
                crate::stage::image_round(b, r, &image, 1.0, b.px(8.0));
            }
            crate::panel::specimens::desktop_bars(app, b, r, entry.desktop.bars());
            let count = entry
                .desktop
                .widgets()
                .get("instances")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            if let Ok(layout) = serde_json::from_value::<mypaper_widgets::layout::Layout>(
                entry.desktop.widgets().clone(),
            ) && let Some(monitor) = app.display()
            {
                let grid = crate::stage::grid_for(&layout, monitor);
                let scale = r.width() / (monitor.rect.width as f32 / (monitor.dpi as f32 / 96.0));
                for mut instance in crate::pages::widgets::here(app, &layout) {
                    let at = grid.rect(instance.cell, instance.size);
                    let frame = Rect::new(
                        r.left + at.x as f32 * scale,
                        r.top + at.y as f32 * scale,
                        at.width as f32 * scale,
                        at.height as f32 * scale,
                    );
                    instance.appearance = instance
                        .appearance
                        .over(&layout.appearance.over(&mypaper_widgets::look::defaults()));
                    if let Some(widget) = app.registry.get(&instance.widget) {
                        widgets::face(app, b, &instance, &widget.manifest, "", "", frame);
                    }
                }
            }
            c.y = r.bottom + b.px(10.0);
            notice(b, c, &entry.detail);
            notice(
                b,
                c,
                &format!(
                    "{} bars · {count} widgets",
                    crate::pages::bars::PRESETS
                        .iter()
                        .find(
                            |(id, _)| entry.desktop.bars().get("preset").and_then(Value::as_str)
                                == Some(id)
                        )
                        .map_or("Custom", |(_, name)| *name)
                ),
            );
        });
    }
}

fn search(app: &App, b: &mut B, c: &mut Col) {
    title(
        b,
        c,
        "Search",
        "Settings, actions and everything on your desktop",
    );
    super::controls::search(app, b, c);
    let results = crate::search::results(app);
    let query = app.text(&FieldId::Palette).trim().to_lowercase();
    let words: Vec<_> = query.split_whitespace().collect();
    let wallpapers: Vec<_> = app
        .library
        .items
        .iter()
        .enumerate()
        .filter(|(_, w)| {
            !query.is_empty()
                && !w.hidden
                && words
                    .iter()
                    .all(|word| format!("wallpaper {}", w.haystack()).contains(word))
        })
        .take(6)
        .map(|(i, _)| i)
        .collect();
    let widgets: Vec<_> = app
        .registry
        .widgets
        .iter()
        .filter(|(_, w)| {
            !query.is_empty()
                && words.iter().all(|word| {
                    format!(
                        "widget {} {}",
                        w.manifest.name,
                        w.manifest.description.as_deref().unwrap_or("")
                    )
                    .to_lowercase()
                    .contains(word)
                })
        })
        .take(8)
        .collect();
    if results.is_empty() && wallpapers.is_empty() && widgets.is_empty() {
        notice(
            b,
            c,
            "Nothing found. Try a setting, wallpaper or widget name.",
        );
    }
    for name in [
        "Settings",
        "Actions",
        "Wallpapers",
        "Widgets",
        "Integrations",
        "Go to",
        "Recent",
    ] {
        if name == "Wallpapers" {
            if !wallpapers.is_empty() {
                group(b, c, "Wallpapers", |b, c| {
                    wallpapers::cards(app, b, c, &wallpapers)
                });
            }
            continue;
        }
        if name == "Widgets" {
            if !widgets.is_empty() {
                group(b, c, "Widgets", |b, c| {
                    for (id, widget) in &widgets {
                        crate::panel::icons::row(
                            app,
                            b,
                            c,
                            id,
                            crate::pages::widgets::glyph_of(id),
                            &widget.manifest.name,
                            "Add to your desktop",
                            false,
                            Action::Panel(Command::PickWidget((*id).clone())),
                        );
                    }
                });
            }
            continue;
        }
        let items: Vec<_> = results
            .iter()
            .enumerate()
            .filter(|(_, e)| e.group == name && e.action != Action::Peek)
            .collect();
        if items.is_empty() {
            continue;
        }
        group(
            b,
            c,
            if name == "Integrations" {
                "Connections"
            } else {
                name
            },
            |b, c| {
                for (i, entry) in items {
                    row(
                        b,
                        c,
                        Some(entry.glyph),
                        &entry.title,
                        "",
                        &entry.section,
                        None,
                        Action::Panel(Command::Search(i)),
                    );
                }
            },
        );
    }
}

pub fn run_search(app: &mut App, index: usize) {
    let Some(entry) = crate::search::results(app).get(index).cloned() else {
        return;
    };
    if let Some(id) = entry
        .id
        .strip_prefix("setting:")
        .and_then(crate::settings::Setting::get)
    {
        app.reveal(id);
        let page = match id.place {
            crate::settings::Place::Look { windows } => {
                if windows {
                    Page::Windows
                } else {
                    Page::Look
                }
            }
            crate::settings::Place::Bars(Some(crate::actions::Bar::Top)) => Page::Top,
            crate::settings::Place::Bars(Some(crate::actions::Bar::Dock)) => Page::Dock,
            crate::settings::Place::Bars(_) | crate::settings::Place::BarsPanel(_) => Page::Bars,
            crate::settings::Place::Settings(section) => section_page(section),
            crate::settings::Place::Page(page) => legacy_page(page),
        };
        app.panel.as_mut().unwrap().page = page;
        let view = key(("studio-panel", page));
        app.nav.more.insert(key(("panel-more", view)));
        return;
    }
    match entry.action {
        Action::Nav(page) => app.panel.as_mut().unwrap().page = legacy_page(page),
        Action::Section(section) => app.panel.as_mut().unwrap().page = section_page(section),
        Action::AddWidget(id) => {
            app.panel.as_mut().unwrap().add_widget = Some(id);
            app.panel.as_mut().unwrap().page = Page::AddWidget;
        }
        Action::SelectIntegration(i) => {
            app.nav.integration = i;
            app.panel.as_mut().unwrap().page = Page::Connection;
        }
        action => app.run(action),
    }
}

fn legacy_page(page: crate::actions::Page) -> Page {
    match page {
        crate::actions::Page::Home => Page::Root,
        crate::actions::Page::Wallpapers => Page::Wallpapers,
        crate::actions::Page::Look => Page::Look,
        crate::actions::Page::Widgets => Page::Widgets,
        crate::actions::Page::Bars => Page::Bars,
        crate::actions::Page::Integrations => Page::Connections,
        crate::actions::Page::Settings => Page::Settings,
    }
}

fn section_page(section: crate::actions::Section) -> Page {
    match section {
        crate::actions::Section::General => Page::General,
        crate::actions::Section::Performance => Page::Performance,
        crate::actions::Section::Desktop => Page::DesktopMenu,
        crate::actions::Section::Keyboard => Page::Keyboard,
        crate::actions::Section::Troubleshoot => Page::Troubleshoot,
        crate::actions::Section::About => Page::About,
    }
}

/// A numeric editor using the same slider drag semantics as the table-driven settings.
pub fn number(
    app: &App,
    b: &mut B,
    c: &mut Col,
    doc: DocId,
    path: &str,
    label: &str,
    range: (f64, f64, f64),
    fallback: f64,
) {
    let value = app
        .get(doc, path)
        .and_then(Value::as_f64)
        .unwrap_or(fallback);
    let h = height(b, 38.0);
    text(
        b,
        Rect::new(c.x, c.y, b.px(92.0), h),
        label,
        12.0,
        400,
        b.p.ink1,
        Align::Start,
    );
    b.slider(
        key(("panel-number", doc, path)),
        Rect::new(c.x + b.px(96.0), c.y, c.w - b.px(96.0), h),
        value,
        range,
        &format!("{value:.2}"),
        b.px(42.0),
        label,
        Action::Slider(Edit::new(doc, path, label)),
    );
    c.y += h;
}

pub fn starter_preview(app: &App, b: &mut B, index: usize, rect: Rect) {
    let Some((wallpaper, preset, items)) = crate::pages::first_run::panel_starter(index) else {
        return;
    };
    b.fill(rect, b.px(6.0), b.p.fill);
    if let Some(path) = app
        .library
        .items
        .iter()
        .find(|item| item.id == wallpaper)
        .and_then(|item| item.preview.as_ref())
        && let Some(image) = app.images.borrow_mut().get(path, (384, 256))
    {
        crate::stage::image_round(b, rect, &image, 1.0, b.px(6.0));
    }
    let mut bars = serde_json::json!({"version":1,"preset":preset});
    if matches!(preset, "macos" | "islands") {
        bars["dock"] = serde_json::json!({"enabled":true});
    }
    crate::panel::specimens::desktop_bars(app, b, rect, &bars);
    for (i, (id, size)) in items.iter().enumerate() {
        let Some(widget) = app.registry.get(id) else {
            continue;
        };
        let instance = Instance {
            instance: format!("starter-{id}"),
            widget: (*id).into(),
            monitor: String::new(),
            cell: [20, i as u32 * 2],
            size: *size,
            settings: Default::default(),
            appearance: Default::default(),
        };
        let frame = Rect::new(
            rect.left + rect.width() * 0.61,
            rect.top + rect.height() * (0.16 + i as f32 * 0.3),
            rect.width() * 0.32,
            rect.height() * 0.27,
        );
        widgets::face(app, b, &instance, &widget.manifest, "", "", frame);
    }
}
