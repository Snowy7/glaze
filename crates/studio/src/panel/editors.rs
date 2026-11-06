mod bars;
mod connections;
pub use bars::{
    bar_path, bar_value, design_name as bar_design_name, insert_module, move_module,
    overview as bars_overview, same_bars,
};
pub use connections::{connect, connection_module, disconnect, hide_connection_widget};
mod look;
mod settings;
pub use settings::add_menu_item;
pub use settings::frame_cap;
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
        Page::AddWidget => {
            if let Some(id) = app.panel.as_ref().unwrap().add_widget.as_deref()
                && !widget_has_room(app, id)
            {
                let layout = crate::pages::widgets::layout(app);
                let make_room = crate::pages::widgets::here(app, &layout)
                    .into_iter()
                    .max_by_key(|instance| instance.size[0] * instance.size[1])
                    .map(|instance| ("Make room", Action::SelectWidget(Some(instance.instance))));
                super::controls::feedback(b, c, "No room", make_room.into_iter().collect());
            }
            widgets::add(app, b, c);
        }
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
        Page::BarDesign => bars::designs(app, b, c),
        Page::Module => bars::module(app, b, c),
        Page::AddModule => bars::add(app, b, c),
        Page::Panels => bars::panels(b, c),
        Page::BarPanel => bars::panel(app, b, c),
        Page::FirstRun => super::first_run::view(app, b, c),
        _ => {}
    }
}

pub fn intercept_page_set(app: &mut App, edit: &Edit, value: &Value) -> bool {
    if edit.doc == DocId::Studio && edit.path.as_ref() == "/add_use_now" {
        app.texts.insert(
            FieldId::Text(DocId::Studio, edit.path.clone()),
            value.to_string(),
        );
        return true;
    }
    bars::intercept_set(app, edit, value) || settings::intercept_set(app, edit, value)
}

pub fn field_initial(app: &App, id: &FieldId) -> Option<String> {
    bars::field_initial(app, id)
}

/// Creation and its room check must use the same display grid as the shell.
pub fn prepare_widget_grid(app: &App) {
    if let Some(monitor) = app.display() {
        let layout = crate::pages::widgets::layout(app);
        app.stage.borrow_mut().content.grid = Some((crate::stage::grid_for(&layout, monitor), 1.0));
    }
}

pub fn widget_has_room(app: &App, id: &str) -> bool {
    let Some(manifest) = app.registry.get(id).map(|w| &w.manifest) else {
        return false;
    };
    let Some(monitor) = app.display() else {
        return false;
    };
    let layout = crate::pages::widgets::layout(app);
    let grid = crate::stage::grid_for(&layout, monitor);
    let size = manifest.size.default;
    if size[0] > grid.cols || size[1] > grid.rows {
        return false;
    }
    let instances = crate::pages::widgets::here(app, &layout);
    (0..=grid.rows - size[1]).any(|row| {
        (0..=grid.cols - size[0]).any(|column| {
            instances.iter().all(|instance| {
                column + size[0] <= instance.cell[0]
                    || instance.cell[0] + instance.size[0] <= column
                    || row + size[1] <= instance.cell[1]
                    || instance.cell[1] + instance.size[1] <= row
            })
        })
    })
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
        (0, 0) => "Top left",
        (1, 0) => "Top",
        (2, 0) => "Top right",
        (0, 1) => "Left",
        (1, 1) => "Center",
        (2, 1) => "Right",
        (0, _) => "Bottom left",
        (1, _) => "Bottom",
        _ => "Bottom right",
    };
    format!(
        "{area}, column {}, row {}",
        instance.cell[0] + 1,
        instance.cell[1] + 1
    )
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
        Some(Page::History) if super::history::can_restore(app) => 64.0,
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
            "Skip remaining steps",
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
                .is_some_and(|id| can_add_widget(app, id) && widget_has_room(app, id)),
        ),
        Page::AddWallpaper => (
            "Cancel",
            open(Page::Wallpapers),
            if wallpapers::use_now(app) {
                "Add and use"
            } else {
                "Add"
            },
            Action::AddCommit(wallpapers::use_now(app)),
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
    let enabled = enabled
        && !super::controls::relevant_docs(state.page)
            .iter()
            .any(|id| app.docs.doc(*id).issue.is_some());
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
    let mut visible_pictures = std::collections::HashSet::new();
    group(b, c, "Desktop changes", |b, c| {
        for (i, entry) in history.entries.iter().enumerate().rev() {
            let r = Rect::new(c.x - b.px(8.0), c.y, c.w + b.px(16.0), height(b, 84.0));
            let k = key(("panel-history", i));
            if history.selected == Some(i) {
                b.fill(r, b.px(8.0), b.p.selected);
            } else {
                b.wash(k, r, b.px(8.0));
            }
            let pic = Rect::new(c.x, c.y + b.px(10.0), b.px(108.0), b.px(61.0));
            desktop_preview(app, b, pic, i, entry, &mut visible_pictures);
            let x = pic.right + b.px(12.0);
            let summary = if entry.summary.is_empty() {
                &entry.title
            } else {
                &entry.summary
            };
            text(
                b,
                Rect::new(x, c.y + b.px(8.0), c.right() - x, height(b, 30.0)),
                summary,
                13.0,
                500,
                b.p.ink1,
                Align::Start,
            );
            let when = format!(
                "{}{}",
                super::history::time_label(entry.at),
                if i + 1 == history.entries.len() {
                    " · Now"
                } else {
                    ""
                }
            );
            text(
                b,
                Rect::new(x, c.y + height(b, 38.0), c.right() - x, height(b, 22.0)),
                &when,
                12.0,
                400,
                b.p.ink3,
                Align::Start,
            );
            b.hit_checked(
                k,
                r,
                Role::RadioButton,
                &format!("{summary}, {when}, {}", entry.detail),
                Some(history.selected == Some(i)),
                Action::Panel(Command::History(i)),
                Behavior::Click,
            );
            b.focus_ring(k, r, b.px(8.0));
            c.y = r.bottom;
        }
    });
    if let Some((i, entry)) = history
        .selected
        .and_then(|i| history.entries.get(i).map(|entry| (i, entry)))
    {
        group(b, c, "Selected desktop", |b, c| {
            let r = Rect::new(c.x, c.y + b.px(6.0), c.w, c.w * 9.0 / 16.0);
            desktop_preview(app, b, r, i, entry, &mut visible_pictures);
            let count = entry
                .desktop
                .widgets()
                .get("instances")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            c.y = r.bottom + b.px(10.0);
            notice(b, c, &entry.detail);
            notice(
                b,
                c,
                &format!(
                    "{} bars · {count} widgets across all displays",
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
    history
        .pictures
        .borrow_mut()
        .retain(|key, _| visible_pictures.contains(key));
}

fn desktop_preview(
    app: &App,
    b: &mut B,
    r: Rect,
    index: usize,
    entry: &super::history::Entry,
    visible: &mut std::collections::HashSet<(usize, u32, u32)>,
) {
    if b.clip_rect().is_some_and(|clip| !r.intersects(&clip)) {
        return;
    }
    let size = (
        r.width().ceil().max(1.0) as u32,
        r.height().ceil().max(1.0) as u32,
    );
    let key = (index, size.0, size.1);
    visible.insert(key);
    let history = app.panel.as_ref().unwrap().history.borrow();
    let cached = history.pictures.borrow().get(&key).cloned();
    let picture = cached.or_else(|| {
        let picture = super::history::picture(app, &entry.desktop, size, entry.at)?;
        history.pictures.borrow_mut().insert(key, picture.clone());
        Some(picture)
    });
    b.fill(r, b.px(6.0), b.p.fill);
    if let Some(picture) = picture {
        crate::stage::image_round(b, r, &picture, 1.0, b.px(6.0));
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
        "On your desktop",
        "Actions",
        "Wallpapers",
        "Widgets",
        "Integrations",
        "History",
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
            crate::settings::Place::Bars(_) => Page::Bars,
            crate::settings::Place::BarsPanel(_) => Page::BarPanel,
            crate::settings::Place::Settings(section) => section_page(section),
            crate::settings::Place::Page(page) => legacy_page(page),
        };
        app.panel.as_mut().unwrap().page = page;
        let view = key(("studio-panel", page));
        app.nav.more.insert(key(("panel-more", view)));
        return;
    }
    let reveal = entry.reveal.clone();
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
        Action::SelectWidget(Some(id)) => {
            if let Some(instance) = crate::pages::widgets::layout(app).get(&id) {
                app.nav.display = app
                    .monitors
                    .iter()
                    .position(|m| m.device == instance.monitor)
                    .unwrap_or(0);
            }
            app.run(Action::SelectWidget(Some(id)));
        }
        Action::Panel(Command::History(index)) => {
            app.panel.as_mut().unwrap().page = Page::History;
            app.run(Action::Panel(Command::History(index)));
        }
        action => app.run(action),
    }
    if let Some(label) = reveal {
        let page = app.panel.as_ref().unwrap().page;
        app.panel.as_mut().unwrap().search_match = Some((page, label));
        app.nav
            .more
            .insert(key(("panel-more", key(("studio-panel", page)))));
        app.pending_reveal = Some("panel-search-match");
        app.anchors.borrow_mut().remove("panel-search-match");
        app.flash = Some(("panel-search-match", std::time::Instant::now()));
    }
}

/// Dynamic manifest controls use one reusable anchor, never leaked strings for search IDs.
pub fn reveal_search(app: &App, b: &mut B, page: Page, first_target: usize) {
    let Some((on_page, label)) = app
        .panel
        .as_ref()
        .and_then(|state| state.search_match.as_ref())
    else {
        return;
    };
    if *on_page != page {
        return;
    }
    let target = b
        .targets
        .iter()
        .enumerate()
        .skip(first_target)
        .find_map(|(index, target)| {
            let hit = b.list.hits.iter().find(|hit| hit.id == index as u32 + 1)?;
            (hit.label.as_ref() == label
                || hit.label.starts_with(&format!("{label}:"))
                || hit.label.starts_with(&format!("{label},")))
            .then_some(target.rect)
        })
        .or_else(|| {
            b.list.ops.iter().find_map(|op| match op {
                mypaper_ui::Op::Text { rect, text, .. } if text.as_ref() == label => Some(*rect),
                _ => None,
            })
        });
    if let Some(rect) = target {
        crate::ui::anchor(
            app,
            "panel-search-match",
            key(("studio-panel", page)),
            rect.top,
        );
        if app
            .flash
            .is_some_and(|(id, at)| id == "panel-search-match" && at.elapsed().as_secs_f32() < 2.0)
        {
            b.stroke(rect.inflate(b.px(2.0)), b.px(7.0), b.px(2.0), b.p.ink2);
        }
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
    let source = if doc == DocId::Bars {
        bars::bar_value(app, path)
    } else {
        app.get(doc, path).cloned()
    };
    let value = source.as_ref().and_then(Value::as_f64).unwrap_or(fallback);
    super::material::row(
        b,
        c,
        label,
        value,
        range,
        &super::controls::unit(
            value,
            if matches!(label, "Corners" | "Padding" | "Text size") {
                "px"
            } else {
                ""
            },
        ),
        Edit::new(doc, path, label),
    );
}

pub fn starter_preview(app: &App, b: &mut B, index: usize, rect: Rect) {
    // Keep the one visible specimen at the panel's actual pixel resolution.
    let width = rect.width().ceil().max(1.0) as u32;
    let size = (
        width,
        (rect.height() * width as f32 / rect.width().max(1.0))
            .ceil()
            .max(1.0) as u32,
    );
    let taskbar = match app.overlay {
        Some(crate::app::Overlay::FirstRun { taskbar, .. }) => taskbar,
        _ => 0,
    };
    let key = format!("starter:{index}:{taskbar}:{size:?}");
    let cached = app
        .panel
        .as_ref()
        .unwrap()
        .specimens
        .borrow()
        .get(&key)
        .cloned();
    let picture = cached.or_else(|| {
        let picture = super::history::starter_picture(app, index, size, taskbar)?;
        super::specimens::remember(app, key, &picture);
        Some(picture)
    });
    b.fill(rect, b.px(6.0), b.p.fill);
    if let Some(picture) = picture {
        crate::stage::image_round(b, rect, &picture, 1.0, b.px(6.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn widget_room_state_and_creation_use_the_actual_display_extent() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(crate::panel::State::default());
        app.monitors.truncate(1);
        let grid =
            crate::stage::grid_for(&crate::pages::widgets::layout(&app), app.display().unwrap());
        assert!(grid.cols > 30);
        let widgets = serde_json::json!({"version":2,"instances":[{"instance":"cover","widget":"clock","monitor":"","cell":[0,0],"size":[24,grid.rows]}]});
        app.docs = crate::docs::Docs::fixture(["{}", "{}", &widgets.to_string(), "{}"]);
        assert!(widget_has_room(&app, "clock"));
        app.run(Action::AddWidget("clock".into()));
        let layout = crate::pages::widgets::layout(&app);
        assert_eq!(layout.instances.len(), 2);
        assert!(layout.instances[1].cell[0] >= 24);
        let widgets = serde_json::json!({"version":2,"instances":[{"instance":"cover","widget":"clock","monitor":"","cell":[0,0],"size":[grid.cols,grid.rows]}]});
        app.docs = crate::docs::Docs::fixture(["{}", "{}", &widgets.to_string(), "{}"]);
        assert!(!widget_has_room(&app, "clock"));
        app.run(Action::AddWidget("clock".into()));
        assert_eq!(crate::pages::widgets::layout(&app).instances.len(), 1);
        assert!(
            app.toast
                .as_ref()
                .unwrap()
                .action
                .as_ref()
                .is_some_and(|(label, _)| label == "Make room")
        );
    }
}
