//! Kit-only browser layout. The row range is computed before building paint operations: a
//! 100,000-entry folder produces the same amount of drawing work as a small folder.
use crate::{
    app::*,
    model::{self, View},
    shell,
};
use anyhow::Result;
use mypaper_ui::{icons::Icon, *};
use std::time::Instant;

pub fn column_widths(app: &App, width: f32) -> Vec<f32> {
    let s = app.surface.scale();
    if app.preferences.widths.len() == 4
        && app
            .preferences
            .widths
            .iter()
            .all(|w| w.is_finite() && *w >= 60.0 && *w <= 700.0)
    {
        app.preferences
            .widths
            .iter()
            .map(|w| (w * s).round())
            .collect()
    } else {
        vec![
            (width - 470.0 * s).max(220.0 * s),
            190.0 * s,
            170.0 * s,
            110.0 * s,
        ]
    }
}

fn field(
    f: &mut Frame<'_>,
    well: Rect,
    value: &TextField,
    placeholder: &str,
    active: bool,
    icon: Option<Icon>,
) -> Rect {
    let inner = f.field_frame(
        well,
        placeholder,
        value.text().unwrap_or_default().is_empty(),
        active,
        icon,
    );
    let line = f.line(TextRole::Body).min(inner.height());
    let inner = Rect::new(
        inner.left,
        (inner.top + (inner.height() - line) / 2.0).round(),
        inner.width(),
        line,
    );
    value.set_rect(inner);
    f.push(Op::TextField {
        rect: inner,
        field: value.clone(),
        revision: value.revision(),
    });
    if active && let Some(caret) = value.caret() {
        f.fill(caret, 0.0, f.theme.color.label);
    }
    inner
}

pub fn draw(app: &mut App, ui: &mut Ui) -> Result<()> {
    let (width, height) = app.surface.client_size();
    if width == 0 || height == 0 {
        app.prepare_backdrop(Rect::default());
        return Ok(());
    }
    let bounds = Rect::new(0.0, 0.0, width as f32, height as f32);
    let glass = app.prepare_backdrop(bounds);
    let s = app.surface.scale();
    let now = Instant::now();
    let px = |value: f32| (value * s).round();
    let theme = &app.theme;
    let color = theme.color;
    let mut f = Frame::new(&mut ui.text, theme, s, &app.interaction);
    f.surface(bounds, glass);
    let (title, rest) = bounds.take_top(px(46.0), 0.0);
    let (navigation, rest) = rest.take_top(px(58.0), 0.0);
    let (status, body) = rest.take_bottom(px(32.0), 0.0);
    let (sidebar, main) = body.take_left(px(208.0), 0.0);
    let (commands, main) = main.take_top(px(52.0), 0.0);
    let inset = px(16.0);

    // Our title area keeps native drag, resize, Aero Snap and caption button semantics.
    f.icon(
        Rect::new(px(18.0), title.top + px(10.0), px(26.0), px(26.0)),
        Icon::Folder,
        21.0,
        color.accent,
    );
    f.text_in(
        Rect::new(px(52.0), title.top, px(120.0), title.height()),
        "Files",
        TextRole::Headline,
        color.label,
        Align::Start,
    );
    let buttons = title.take_right(px(138.0), 0.0).0;
    let mut chrome = vec![(title, Chrome::Caption)];
    for (index, id, icon, label, kind) in [
        (0, MINIMIZE, Icon::Remove, "Minimize", Chrome::Minimize),
        (1, MAXIMIZE, Icon::Window, "Maximize", Chrome::Maximize),
        (2, CLOSE, Icon::Close, "Close", Chrome::Close),
    ] {
        let rect = Rect::new(
            buttons.left + px(46.0) * index as f32,
            buttons.top,
            px(46.0),
            buttons.height(),
        );
        f.icon_button(id, rect, icon, 15.0, label, false);
        chrome.push((rect, kind));
    }
    app.surface.set_chrome(&chrome);

    let nav = navigation.inset(Insets::xy(px(14.0), px(9.0)));
    let (moves, nav) = nav.take_left(px(122.0), px(10.0));
    for (index, id, icon, label, enabled) in [
        (
            0,
            BACK,
            Icon::ChevronLeft,
            "Back (Alt+Left)",
            app.history.can_back(),
        ),
        (
            1,
            FORWARD,
            Icon::ChevronRight,
            "Forward (Alt+Right)",
            app.history.can_forward(),
        ),
        (
            2,
            UP,
            Icon::ChevronUp,
            "Up (Alt+Up)",
            app.location.as_ref().is_some_and(|l| l.parent.is_some()),
        ),
    ] {
        let rect = Rect::new(
            moves.left + px(40.0) * index as f32,
            moves.top,
            px(36.0),
            moves.height(),
        );
        f.button(id, rect, "", Some(icon), ButtonKind::Plain, enabled);
        if let Some(hit) = f.list.hits.last_mut() {
            hit.label = label.into();
        }
    }
    let (search, address) = nav.take_right(px(260.0).min(nav.width() * 0.36), px(12.0));
    let search_inner = field(
        &mut f,
        search,
        &app.search,
        "Search files",
        app.field == Field::Search,
        Some(Icon::Search),
    );
    f.hit(
        SEARCH,
        search,
        Role::TextInput,
        "Search files (Ctrl+E)",
        true,
        None,
    );
    let address_inner;
    let mut crumbs = Vec::new();
    if app.field == Field::Address {
        address_inner = field(
            &mut f,
            address,
            &app.address,
            "Folder path or shell: location",
            true,
            None,
        );
    } else {
        address_inner = f.field_frame(address, "", false, false, None);
        f.hit(
            ADDRESS,
            address,
            Role::TextInput,
            "Address (Ctrl+L)",
            true,
            None,
        );
        f.icon(
            Rect::new(
                address_inner.left,
                address_inner.top,
                px(26.0),
                address_inner.height(),
            ),
            Icon::Folder,
            16.0,
            color.secondary,
        );
        let mut x = address_inner.left + px(30.0);
        if let Some(location) = &app.location {
            let components = breadcrumbs(location);
            // Keep the current folder visible in narrow windows; Ctrl+L exposes the full path.
            let available = address_inner.right - x;
            let estimated: f32 = components
                .iter()
                .map(|(name, _)| name.chars().count() as f32 * px(7.0) + px(32.0))
                .sum();
            let components = if estimated > available && components.len() > 2 {
                components
                    .into_iter()
                    .rev()
                    .take(2)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect()
            } else {
                components
            };
            for (name, path) in components {
                let measured = f.text.measure(&name, &f.font(TextRole::Body)).0;
                let w = (measured + px(20.0)).min((address_inner.right - x).max(0.0));
                if w < px(22.0) {
                    break;
                }
                let rect = Rect::new(x, address_inner.top, w, address_inner.height());
                let id = CRUMB + crumbs.len() as u32;
                f.wash(id, rect, px(5.0), true);
                f.text_in(
                    rect.inset(Insets::xy(px(8.0), 0.0)),
                    &name,
                    TextRole::Body,
                    color.label,
                    Align::Start,
                );
                f.hit(id, rect, Role::Button, &name, true, None);
                crumbs.push(path);
                x += w;
                if x + px(18.0) < address_inner.right {
                    f.icon(
                        Rect::new(x, address_inner.top, px(18.0), address_inner.height()),
                        Icon::ChevronRight,
                        12.0,
                        color.tertiary,
                    );
                    x += px(18.0);
                }
            }
        } else {
            f.text_in(
                address_inner.offset(px(30.0), 0.0),
                "Opening folder…",
                TextRole::Body,
                color.secondary,
                Align::Start,
            );
        }
    }

    f.fill(sidebar, 0.0, color.well.with_alpha(color.well.a * 0.55));
    let (side_title, side_list) = sidebar
        .inset(Insets::xy(px(12.0), px(10.0)))
        .take_top(px(32.0), px(4.0));
    f.text_in(
        side_title.inset(Insets::xy(px(10.0), 0.0)),
        "Places",
        TextRole::Subheadline,
        color.tertiary,
        Align::Start,
    );
    let side_h = px(38.0);
    let side_offset = app.sidebar_scroll.offset(now);
    f.scroll(side_list, &app.sidebar_scroll, now, |f, origin| {
        for index in
            model::visible_range(app.sidebar.len(), side_offset, side_list.height(), side_h)
        {
            let (name, path, icon) = &app.sidebar[index];
            let rect = Rect::new(
                origin.x,
                origin.y + index as f32 * side_h,
                side_list.width(),
                side_h - px(2.0),
            );
            let selected = app
                .location
                .as_ref()
                .is_some_and(|l| l.path.eq_ignore_ascii_case(path));
            f.row(
                &ListRow {
                    id: SIDEBAR + index as u32,
                    lead: Lead::Icon(*icon),
                    title: name,
                    subtitle: None,
                    accessory: Accessory::None,
                    selected,
                    enabled: true,
                },
                rect,
            );
        }
        app.sidebar.len() as f32 * side_h
    });

    let command_inner = commands.inset(Insets::xy(inset, px(8.0)));
    let (modes, mut actions) = command_inner.take_right(px(114.0), px(10.0));
    let new_width = f.button_size("New folder", true).0;
    let (new, remaining) = actions.take_left(new_width, px(12.0));
    actions = remaining;
    f.button(
        NEW,
        new,
        "New folder",
        Some(Icon::Add),
        ButtonKind::Plain,
        !app.busy && app.location.as_ref().is_some_and(|l| l.filesystem),
    );
    for (id, icon, label, enabled) in [
        (
            CUT,
            Icon::Edit,
            "Cut (Ctrl+X)",
            !app.selection.items.is_empty(),
        ),
        (
            COPY,
            Icon::Copy,
            "Copy (Ctrl+C)",
            !app.selection.items.is_empty(),
        ),
        (
            PASTE,
            Icon::Clipboard,
            "Paste (Ctrl+V)",
            app.location.as_ref().is_some_and(|l| l.filesystem) && clipboard_has_files(),
        ),
        (
            RENAME,
            Icon::Notes,
            "Rename (F2)",
            app.selection.items.len() == 1,
        ),
        (
            DELETE,
            Icon::Delete,
            "Recycle (Delete)",
            !app.selection.items.is_empty(),
        ),
    ] {
        let (rect, rest) = actions.take_left(px(36.0), px(2.0));
        actions = rest;
        f.button(
            id,
            rect,
            "",
            Some(icon),
            ButtonKind::Plain,
            enabled && !app.busy,
        );
        if let Some(hit) = f.list.hits.last_mut() {
            hit.label = label.into();
        }
    }
    let (detail, modes) = modes.take_left(px(36.0), px(3.0));
    let (icons, more) = modes.take_left(px(36.0), px(3.0));
    f.button(
        DETAILS,
        detail,
        "",
        Some(Icon::Grip),
        if app.preferences.view == View::Details {
            ButtonKind::Tinted
        } else {
            ButtonKind::Plain
        },
        true,
    );
    if let Some(hit) = f.list.hits.last_mut() {
        hit.label = "Details view".into();
    }
    f.button(
        ICONS,
        icons,
        "",
        Some(Icon::Apps),
        if app.preferences.view == View::Icons {
            ButtonKind::Tinted
        } else {
            ButtonKind::Plain
        },
        true,
    );
    if let Some(hit) = f.list.hits.last_mut() {
        hit.label = "Icons view".into();
    }
    f.icon_button(MORE, more, Icon::More, 18.0, "More actions", false);

    let (heading, content) = main.take_top(px(44.0), 0.0);
    let content = content.inset(Insets::xy(inset, px(3.0)));
    let widths = column_widths(app, content.width());
    let mut boundaries = Vec::new();
    if app.preferences.view == View::Details {
        f.separator(
            heading.left + inset,
            heading.right - inset,
            heading.bottom - px(1.0),
        );
        let mut x = heading.left + inset;
        for (index, name) in ["Name", "Date modified", "Type", "Size"]
            .into_iter()
            .enumerate()
        {
            let rect = Rect::new(
                x,
                heading.top + px(4.0),
                widths[index],
                heading.height() - px(8.0),
            );
            let label = if app.sort.column as usize == index {
                format!("{name}  {}", if app.sort.descending { "↓" } else { "↑" })
            } else {
                name.into()
            };
            let id = HEADER + index as u32;
            f.wash(id, rect, px(5.0), true);
            f.text_in(
                rect.inset(Insets::xy(px(10.0), 0.0)),
                &label,
                TextRole::Subheadline,
                color.secondary,
                Align::Start,
            );
            f.hit(id, rect, Role::Button, &label, true, None);
            x += widths[index];
            if index < 3 {
                boundaries.push(x);
            }
        }
    } else {
        let title = app.location.as_ref().map_or("Files", |l| l.name.as_str());
        f.text_in(
            heading.inset(Insets::xy(inset, 0.0)),
            title,
            TextRole::Title3,
            color.label,
            Align::Start,
        );
    }

    let mut rows = Vec::new();
    let (row_height, columns) = if app.preferences.view == View::Details {
        (px(34.0), 1)
    } else {
        (
            px(132.0),
            (content.width() / px(120.0)).floor().max(1.0) as usize,
        )
    };
    let count = app.shown.len().div_ceil(columns);
    let offset = app.scroll.offset(now);
    let icon_size = ((if app.preferences.view == View::Icons {
        72.0
    } else {
        20.0
    }) * s)
        .round() as i32;
    if app.shown.is_empty() {
        if let Some(error) = &app.error {
            f.empty(
                content,
                Some(Icon::Warning),
                "Couldn’t open this location",
                Some(error),
            );
        } else if app.loading {
            f.empty(
                content,
                Some(Icon::Folder),
                "Opening folder…",
                Some("Files will appear as they arrive."),
            );
        } else if !app.query_text.is_empty() {
            f.empty(
                content,
                Some(Icon::Search),
                "No matching files",
                Some("Try another name, or clear the search."),
            );
        } else {
            f.empty(
                content,
                Some(Icon::Folder),
                "This folder is empty",
                Some("Create a folder or paste files here."),
            );
        }
    }
    f.scroll(content, &app.scroll, now, |f, origin| {
        for row in model::visible_range(count, offset, content.height(), row_height) {
            for column in 0..columns {
                let index = row * columns + column;
                let Some(entry) = app.shown.get(index) else {
                    break;
                };
                let cell_width = content.width() / columns as f32;
                let rect = Rect::new(
                    origin.x + column as f32 * cell_width,
                    origin.y + row as f32 * row_height,
                    cell_width,
                    row_height - px(2.0),
                );
                rows.push((index, rect.intersect(content)));
                let id = ITEM + index as u32;
                let selected = app.selection.items.contains(&index);
                if selected {
                    f.fill(rect, px(5.0), color.accent_wash);
                }
                f.wash(id, rect, px(5.0), true);
                if app.selection.focus == Some(index) && app.interaction.focus_visible {
                    f.focus_ring(id, rect, px(5.0));
                }
                let decoration = app
                    .decorations
                    .get(&(entry.path.clone(), icon_size))
                    .and_then(Option::as_ref);
                let name = decoration.map_or(entry.name.as_str(), |d| d.name.as_str());
                let label_color = if entry.hidden {
                    color.secondary
                } else {
                    color.label
                };
                let image = decoration.and_then(|d| d.image.as_ref());
                let icon = if entry.directory {
                    Icon::Folder
                } else {
                    Icon::File
                };
                if app.preferences.view == View::Details {
                    let icon_rect =
                        Rect::new(rect.left + px(8.0), rect.top + px(6.0), px(20.0), px(20.0));
                    if let Some(image) = image {
                        f.push(Op::Image {
                            rect: icon_rect,
                            image: image.clone(),
                            opacity: if entry.hidden { 0.6 } else { 1.0 },
                            source: None,
                        });
                    } else {
                        f.icon(
                            icon_rect,
                            icon,
                            18.0,
                            if entry.directory {
                                color.accent
                            } else {
                                color.secondary
                            },
                        );
                    }
                    let name_rect = Rect::new(
                        rect.left + px(38.0),
                        rect.top,
                        widths[0] - px(48.0),
                        rect.height(),
                    );
                    f.text_in(name_rect, name, TextRole::Body, label_color, Align::Start);
                    let mut x = rect.left + widths[0];
                    f.text_in(
                        Rect::new(x + px(10.0), rect.top, widths[1] - px(20.0), rect.height()),
                        &shell::modified_label(entry.modified),
                        TextRole::Callout,
                        color.secondary,
                        Align::Start,
                    );
                    x += widths[1];
                    let kind = decoration
                        .filter(|d| !d.kind.is_empty())
                        .map_or(entry.kind.as_str(), |d| d.kind.as_str());
                    f.text_in(
                        Rect::new(x + px(10.0), rect.top, widths[2] - px(20.0), rect.height()),
                        kind,
                        TextRole::Callout,
                        color.secondary,
                        Align::Start,
                    );
                    x += widths[2];
                    let size = app
                        .sizes
                        .get(&entry.path)
                        .copied()
                        .or(entry.size)
                        .map(model::size_label)
                        .unwrap_or_default();
                    f.text_in(
                        Rect::new(x + px(10.0), rect.top, widths[3] - px(24.0), rect.height()),
                        &size,
                        TextRole::Callout,
                        color.secondary,
                        Align::End,
                    );
                } else {
                    let icon_rect = Rect::new(
                        (rect.left + (rect.width() - px(72.0)) / 2.0).round(),
                        rect.top + px(10.0),
                        px(72.0),
                        px(72.0),
                    );
                    if let Some(image) = image {
                        let factor = (icon_rect.width() / image.width as f32)
                            .min(icon_rect.height() / image.height as f32);
                        let w = image.width as f32 * factor;
                        let h = image.height as f32 * factor;
                        let image_rect = Rect::new(
                            (icon_rect.left + (icon_rect.width() - w) / 2.0).round(),
                            (icon_rect.top + (icon_rect.height() - h) / 2.0).round(),
                            w,
                            h,
                        );
                        f.push(Op::Image {
                            rect: image_rect,
                            image: image.clone(),
                            opacity: if entry.hidden { 0.6 } else { 1.0 },
                            source: None,
                        });
                    } else {
                        f.icon(
                            icon_rect,
                            icon,
                            48.0,
                            if entry.directory {
                                color.accent
                            } else {
                                color.secondary
                            },
                        );
                    }
                    f.paragraph(
                        rect.left + px(7.0),
                        rect.top + px(88.0),
                        rect.width() - px(14.0),
                        name,
                        TextRole::Callout,
                        label_color,
                        2,
                    );
                }
                f.hit(id, rect, Role::ListBoxOption, name, true, Some(selected));
            }
        }
        count as f32 * row_height
    });
    if let Some(Drag::Rubber { start, now, .. }) = &app.drag {
        let rubber = Rect {
            left: start.x.min(now.x),
            right: start.x.max(now.x),
            top: start.y.min(now.y),
            bottom: start.y.max(now.y),
        }
        .intersect(content);
        f.fill(rubber, 0.0, color.accent_wash);
    }

    let status_inner = status.inset(Insets::xy(px(18.0), 0.0));
    let count = if !app.query_text.is_empty() {
        format!(
            "{} results · {}",
            app.shown.len(),
            if app.indexed {
                "Indexed files"
            } else {
                "This folder"
            }
        )
    } else if app.loading {
        format!("{} items · Loading…", app.shown.len())
    } else {
        format!("{} items", app.shown.len())
    };
    let selected = if app.selection.items.is_empty() {
        String::new()
    } else {
        format!("  ·  {} selected", app.selection.items.len())
    };
    f.text_in(
        status_inner,
        &format!("{count}{selected}"),
        TextRole::Footnote,
        color.secondary,
        Align::Start,
    );
    if let Some(error) = app.error.as_ref().filter(|_| !app.shown.is_empty()) {
        f.text_in(
            status_inner,
            error,
            TextRole::Footnote,
            color.destructive,
            Align::End,
        );
    } else if !app.status.is_empty() {
        f.text_in(
            status_inner,
            &app.status,
            TextRole::Footnote,
            color.secondary,
            Align::End,
        );
    }

    let mut edit = Rect::default();
    if matches!(app.field, Field::Rename(_) | Field::NewFolder) {
        f.fill(bounds, 0.0, color.shadow.with_alpha(0.30));
        let panel = bounds.place(
            px(440.0),
            px(if app.error.is_some() { 214.0 } else { 182.0 }),
            Align::Center,
            Align::Center,
        );
        f.fill(panel, px(14.0), color.elevated);
        let inner = panel.inset(Insets::all(px(22.0)));
        let (heading, inner) = inner.take_top(px(32.0), px(10.0));
        f.text_in(
            heading,
            if app.field == Field::NewFolder {
                "New folder"
            } else {
                "Rename"
            },
            TextRole::Title3,
            color.label,
            Align::Start,
        );
        let (well, mut buttons) = inner.take_top(px(38.0), px(16.0));
        edit = field(&mut f, well, &app.editor, "Name", true, None);
        f.hit(EDIT_FIELD, well, Role::TextInput, "Name", true, None);
        if let Some(error) = &app.error {
            let (notice, rest) = buttons.take_top(px(24.0), px(8.0));
            f.text_in(
                notice,
                error,
                TextRole::Footnote,
                color.destructive,
                Align::Start,
            );
            buttons = rest;
        }
        let (save, remaining) = buttons.take_right(px(96.0), px(8.0));
        let cancel = remaining.take_right(px(86.0), 0.0).0;
        f.button(
            SUBMIT,
            save,
            if app.field == Field::NewFolder {
                "Create"
            } else {
                "Save"
            },
            None,
            ButtonKind::Filled,
            true,
        );
        f.button(CANCEL, cancel, "Cancel", None, ButtonKind::Plain, true);
        // Modal hits exclude the obscured file list.
        f.list
            .hits
            .retain(|h| h.id == SUBMIT || h.id == CANCEL || h.id == EDIT_FIELD);
    }
    app.geometry = Geometry {
        viewport: content,
        address: address_inner,
        search: search_inner,
        edit,
        columns: boundaries,
        rows,
        crumbs,
        row: row_height,
        grid_columns: columns,
    };
    f.list.focus = match app.field {
        Field::Address => Some(ADDRESS),
        Field::Search => Some(SEARCH),
        Field::Rename(_) | Field::NewFolder => Some(EDIT_FIELD),
        Field::None => app.selection.focus.map(|index| ITEM + index as u32),
    }
    .filter(|id| f.list.hits.iter().any(|hit| hit.id == *id));
    let list = f.list;
    if glass {
        app.layered.present(&mut app.surface, ui, list)?;
    } else {
        app.surface
            .present(ui, app.surface.origin(), (width, height), list)?;
    }
    app.request_images();
    Ok(())
}

fn breadcrumbs(location: &shell::Location) -> Vec<(String, String)> {
    if !location.filesystem {
        return vec![(location.name.clone(), location.path.clone())];
    }
    let path = std::path::Path::new(&location.path);
    let mut current = std::path::PathBuf::new();
    let mut result: Vec<(String, String)> = Vec::new();
    for component in path.components() {
        current.push(component);
        if matches!(component, std::path::Component::RootDir) {
            if let Some(last) = result.last_mut() {
                last.1 = current.to_string_lossy().into_owned();
            }
            continue;
        }
        result.push((
            component.as_os_str().to_string_lossy().into_owned(),
            current.to_string_lossy().into_owned(),
        ));
    }
    result
}
