//! The launcher: a search field (windowless RichEdit, so IME, Arabic and CJK input work) over
//! the apps the Start menu lists (`mypaper_bars::flyouts::index`, cached in `apps.json`).
//! Typing filters them; Up and Down move the choice, Enter or a click runs it. With nothing
//! typed it lists what was run most recently (`launcher.json`), then the rest by name.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use mypaper_bars::files;
use mypaper_bars::flyouts::index::{self, App, History};
use mypaper_bars::system;
use mypaper_ui::icons::Icon;
use mypaper_ui::layout::{self, Align, Stack};
use mypaper_ui::{
    Accessory, ButtonKind, Frame, HitId, Image, Lead, ListRow, Op, Rect, Rgba, Role, Scroll, TextField,
    TextRole,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_NEXT, VK_PRIOR, VK_UP};

use super::kit::Kit;

/// The panel padding (the kit theme's `metrics.pad`), design px.
const PAD: f32 = 12.0;
use super::{Cx, Effect, LIST, Panel, PanelCommand, Reply};
use mypaper_bars::config::{LauncherLayout, Pinned};
use mypaper_ui::Entry;

/// Result rows the list shows before it scrolls.
const ROWS: usize = 8;
const ALL: HitId = 10;
const SETTINGS: HitId = 11;
const POWER: HitId = 12;
const CLIPBOARD: HitId = 13;
const RECOMMENDED: HitId = 20_000;
const CLIP: HitId = 30_000;
/// The search field's height, design px.
const FIELD: f32 = 40.0;
/// How old the app index may be before it is rebuilt in the background.
const STALE: Duration = Duration::from_secs(10 * 60);

/// How well `query` (lowercase) matches an app: higher is better, `None` for no match. A
/// prefix of the name beats a word start, which beats a substring, a keyword (the exe's name)
/// and finally typos or letters in order. The edit-distance work is bounded to short names
/// and a length-scaled threshold; arbitrary input cannot turn a keystroke into quadratic work.
pub fn score(query: &str, name: &str, keywords: &[String]) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }
    let name = name.to_lowercase();
    if name == query {
        return Some(1400);
    }
    if name.starts_with(query) {
        return Some(1000 - (name.len() - query.len()).min(200) as u32);
    }
    if name
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(query))
    {
        return Some(800);
    }
    let initials: String = name
        .split(|c: char| !c.is_alphanumeric())
        .filter_map(|w| w.chars().next())
        .collect();
    if initials.starts_with(query) {
        return Some(750);
    }
    if name.contains(query) {
        return Some(600);
    }
    if keywords.iter().any(|k| k.to_lowercase().starts_with(query)) {
        return Some(500);
    }
    let mut letters = name.chars();
    let mut gaps = 0u32;
    let subsequence = query
        .chars()
        .all(|q| {
            for c in letters.by_ref() {
                if c == q {
                    return true;
                }
                gaps += 1;
            }
            false
        })
        .then(|| 300u32.saturating_sub(gaps.min(250)));
    let limit = match query.chars().count() {
        0..=3 => 0,
        4..=7 => 1,
        8..=12 => 2,
        _ => 3,
    };
    if limit == 0 {
        return subsequence;
    }
    let typo = std::iter::once(name.as_str())
        .chain(name.split(|c: char| !c.is_alphanumeric()))
        .chain(keywords.iter().map(String::as_str))
        .filter_map(|candidate| {
            let candidate = candidate.to_lowercase();
            if query.is_ascii() && candidate.is_ascii() {
                edit_distance(query.as_bytes(), candidate.as_bytes(), limit)
            } else {
                edit_distance(
                    &query.chars().collect::<Vec<_>>(),
                    &candidate.chars().collect::<Vec<_>>(),
                    limit,
                )
            }
        })
        .min()
        .map(|distance| 460 - distance as u32 * 60);
    typo.max(subsequence)
}

/// Banded optimal-string-alignment Damerau-Levenshtein: includes adjacent transpositions.
/// Three stack rows avoid per-candidate matrices; length checks bound work before scanning.
fn edit_distance<T: Eq>(a: &[T], b: &[T], limit: usize) -> Option<usize> {
    const MAX: usize = 64;
    if a.len().max(b.len()) > MAX || a.len().abs_diff(b.len()) > limit {
        return None;
    }
    let far = limit + 1;
    let mut previous_previous = [far; MAX + 1];
    let mut previous = [far; MAX + 1];
    for (j, value) in previous.iter_mut().enumerate().take(b.len() + 1) {
        *value = j;
    }
    for i in 1..=a.len() {
        let mut current = [far; MAX + 1];
        current[0] = i;
        let start = i.saturating_sub(limit).max(1);
        let end = (i + limit).min(b.len());
        let mut best = current[0];
        for j in start..=end {
            current[j] = (previous[j] + 1)
                .min(current[j - 1] + 1)
                .min(previous[j - 1] + usize::from(a[i - 1] != b[j - 1]));
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                current[j] = current[j].min(previous_previous[j - 2] + 1);
            }
            best = best.min(current[j]);
        }
        if best > limit {
            return None;
        }
        previous_previous = previous;
        previous = current;
    }
    (previous[b.len()] <= limit).then_some(previous[b.len()])
}

/// The apps for `query`, best first; with no query, the most recently run first.
pub fn results<'a>(apps: &'a [App], history: &History, query: &str) -> Vec<&'a App> {
    let query = query.trim().to_lowercase();
    let now = index::epoch_ms();
    let usage: std::collections::HashMap<_, _> = history
        .recent
        .iter()
        .enumerate()
        .map(|(rank, recent)| (recent.id.as_str(), (rank, recent)))
        .collect();
    let mut scored: Vec<(u32, usize, &App)> = apps
        .iter()
        .filter_map(|app| {
            let (rank, boost) = usage
                .get(app.id.as_str())
                .map_or((usize::MAX, 0), |(rank, r)| {
                    let days = now.saturating_sub(r.last) / 86_400_000;
                    let recency = match days {
                        0 => 40,
                        1..=6 => 20,
                        7..=29 => 10,
                        _ => 0,
                    };
                    (*rank, r.count.min(50) + recency)
                });
            Some((score(&query, &app.name, &app.keywords)? + boost, rank, app))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(a.1.cmp(&b.1))
            .then_with(|| a.2.name.cmp(&b.2.name))
    });
    scored.into_iter().map(|(_, _, app)| app).collect()
}

pub struct Launcher {
    field: Option<TextField>,
    window: Option<HWND>,
    /// Where the search text goes (device px): inside the field's well, after its glyph.
    field_rect: Rect,
    query: String,
    apps: Vec<App>,
    history: History,
    history_path: Option<PathBuf>,
    /// The result chosen with the keyboard.
    chosen: usize,
    /// The pinned grid's first cell (it pages by rows of six; lists scroll).
    top: usize,
    /// The results' (or the clipboard's) scroll position: only that list scrolls.
    scroll: Scroll,
    icons: Vec<(String, Option<Arc<Image>>)>,
    loading: bool,
    compact: bool,
    all_apps: bool,
    pins: Vec<Pinned>,
    pinned_apps: Vec<App>,
    extras: Vec<App>,
    recommended: Vec<App>,
    files: bool,
    clipboard_open: bool,
    clipboard: Option<mypaper_bars::flyouts::clipboard::History>,
    menu_entries: Vec<Entry<PanelCommand>>,
    user: String,
}

/// The search field's well and the text's rectangle inside it at scale `s` (device px): the
/// well spans the panel inside its padding; the text starts after the search glyph.
fn field_rects(s: f32) -> (Rect, Rect) {
    let px = |v: f32| (v * s).round();
    let width = px(Launcher::WIDTH);
    let well = Rect::new(px(PAD), px(PAD), width - 2.0 * px(PAD), px(FIELD));
    let inner = Rect {
        left: well.left + px(10.0) + px(18.0) + px(6.0),
        right: well.right - px(10.0),
        ..well
    };
    // RichEdit draws its line at the top of its rect, so the field's rect is one line (Segoe
    // UI's is 1.33 em), centred in the well like the glyph and the placeholder.
    let line = (FIELD_TEXT * s * 1.33).ceil().min(inner.height());
    let top = (inner.top + (inner.height() - line) / 2.0).round();
    (well, Rect { top, bottom: top + line, ..inner })
}

/// The search text's size, design px (Spotlight's large field).
const FIELD_TEXT: f32 = 17.0;

impl Launcher {
    pub fn new(cx: &mut Cx, window: HWND) -> Self {
        let field_rect = field_rects(cx.scale).1;
        let cache = files::local("apps.json").ok();
        let fresh = cache
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok()?.modified().ok())
            .is_some_and(|at| SystemTime::now().duration_since(at).unwrap_or_default() < STALE);
        let mut apps = cache
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|json| index::parse_cache(&json))
            .unwrap_or_default();
        if apps.is_empty() || !fresh {
            cx.lanes.apps(|builder| Reply::Apps(builder.build()));
        }
        crate::integrations::launcher_entries(&mut apps);
        let history_path = files::local("launcher.json").ok();
        let history = history_path
            .as_deref()
            .map(History::load)
            .unwrap_or_default();
        let config = &cx.bars.config().launcher;
        let pins = config
            .pinned
            .clone()
            .or_else(|| cx.bars.config().dock.pinned.clone())
            .unwrap_or_default();
        let compact = matches!(config.layout.native(), LauncherLayout::Compact);
        let files = config.files;
        let mut launcher = Self {
            field: None,
            window: Some(window),
            field_rect,
            query: String::new(),
            loading: apps.is_empty(),
            apps,
            history,
            history_path,
            chosen: 0,
            top: 0,
            scroll: Scroll::new(&cx.look.theme().motion),
            icons: Vec::new(),
            compact,
            all_apps: false,
            pins,
            pinned_apps: vec![],
            extras: settings(),
            recommended: vec![],
            files,
            clipboard_open: false,
            clipboard: None,
            menu_entries: vec![],
            user: String::new(),
        };
        launcher.resolve_pins();
        launcher
    }

    const WIDTH: f32 = 560.0;

    /// A launcher over `apps` with `field` (tests), as it opens at scale 1.
    #[cfg(test)]
    pub fn with(field: TextField, apps: Vec<App>, history: History) -> Self {
        Self {
            field: Some(field),
            window: None,
            field_rect: field_rects(1.0).1,
            query: String::new(),
            loading: false,
            apps,
            history,
            history_path: None,
            chosen: 0,
            top: 0,
            scroll: Scroll::default(),
            icons: Vec::new(),
            compact: true,
            all_apps: false,
            pins: vec![],
            pinned_apps: vec![],
            extras: vec![],
            recommended: vec![],
            files: false,
            clipboard_open: false,
            clipboard: None,
            menu_entries: vec![],
            user: String::new(),
        }
    }

    #[cfg(test)]
    pub fn win11(field: TextField, apps: Vec<App>) -> Self {
        let mut launcher = Self::with(field, apps, History::default());
        launcher.compact = false;
        launcher.pins = launcher
            .apps
            .iter()
            .take(6)
            .map(|a| Pinned {
                path: a.path.clone(),
                name: Some(a.name.clone()),
            })
            .collect();
        launcher.resolve_pins();
        launcher
    }

    /// The field's rectangle at scale `s` (tests create the field there).
    #[cfg(test)]
    pub fn field_rect(s: f32) -> Rect {
        field_rects(s).1
    }

    /// The search text's size at scale `s`, device px.
    pub fn field_text(s: f32) -> f32 {
        FIELD_TEXT * s
    }

    fn grid(&self) -> bool {
        !self.compact && !self.all_apps && self.query.trim().is_empty() && !self.clipboard_open
    }

    fn resolve_pins(&mut self) {
        self.pinned_apps = self
            .pins
            .iter()
            .map(|pin| resolve_pin(pin, &self.apps, None))
            .collect();
    }

    fn refresh_pins(&mut self, cx: &mut Cx) -> bool {
        let pins: Vec<_> = self
            .pins
            .iter()
            .map(|pin| {
                let described = cx.bars.describe_pin(&pin.path);
                resolve_pin(pin, &self.apps, described.as_ref())
            })
            .collect();
        if pins == self.pinned_apps {
            return false;
        }
        self.pinned_apps = pins;
        self.load_icons(cx);
        true
    }

    fn shown(&self) -> Vec<&App> {
        if self.grid() {
            return self.pinned_apps.iter().collect();
        }
        let mut found = results(&self.apps, &self.history, &self.query);
        if !self.query.trim().is_empty() {
            let extras = results(&self.extras, &self.history, &self.query);
            found.extend(
                extras
                    .iter()
                    .copied()
                    .filter(|a| a.id.starts_with("settings:")),
            );
            found.extend(extras.into_iter().filter(|a| a.id.starts_with("file:")));
        } else if self.all_apps {
            found.sort_by_cached_key(|a| a.name.to_lowercase());
        }
        found
    }

    /// Decodes the icons of the results on screen and a few around them (and keeps no others).
    fn load_icons(&mut self, cx: &mut Cx) {
        let (first, count) = if self.grid() {
            (self.top, 12)
        } else {
            let row = (self.theme_row() * cx.scale).round().max(1.0);
            let first = (self.scroll.offset(Instant::now()) / row) as usize;
            (first.saturating_sub(4), ROWS + 8)
        };
        let wanted: Vec<(String, Option<String>)> = self
            .shown()
            .into_iter()
            .skip(first)
            .take(count)
            .map(|a| (a.id.clone(), a.icon.clone()))
            .collect();
        self.icons = wanted
            .into_iter()
            .map(|(id, path)| {
                let image = cx.image(path.as_deref());
                (id, image)
            })
            .collect();
    }

    /// A result row's height, design px (the kit's single-line row).
    fn theme_row(&self) -> f32 {
        36.0
    }

    fn icon(&self, id: &str) -> Option<Arc<Image>> {
        self.icons
            .iter()
            .find(|(i, _)| i == id)
            .and_then(|(_, image)| image.clone())
    }

    fn launch(&mut self, index: usize) -> Effect {
        let Some(app) = self.shown().get(index).map(|a| (*a).clone()) else {
            return Effect::None;
        };
        if crate::integrations::launcher_action(&app.path) {
            return Effect::Switch(super::Kind::RoutineResult);
        }
        system::open(app.path.clone());
        self.history.record(&app.id, &app.path, index::epoch_ms());
        if let Some(path) = &self.history_path {
            self.history.save(path);
        }
        Effect::Close
    }

    fn choose(&mut self, delta: isize, cx: &mut Cx) -> Effect {
        let count = if self.clipboard_open {
            self.clipboard.as_ref().map_or(0, |h| h.entries.len())
        } else {
            self.shown().len()
        };
        if count == 0 {
            return Effect::None;
        }
        self.chosen = (self.chosen as isize + delta).rem_euclid(count as isize) as usize;
        if self.grid() {
            let row = self.chosen / 6;
            if row < self.top / 6 {
                self.top = row * 6;
            } else if row >= self.top / 6 + 2 {
                self.top = (row - 1) * 6;
            }
        } else {
            // Keep the chosen row in the viewport (it glides there).
            let s = cx.scale;
            let (tops, _) = if self.clipboard_open {
                let row = (self.theme_row() * s).round();
                ((0..count).map(|i| i as f32 * row).collect(), 0.0)
            } else {
                positions(&self.shown(), &self.history, &self.query, s)
            };
            if let Some(&top) = tops.get(self.chosen) {
                let row = (self.theme_row() * s).round();
                self.scroll.reveal(top, top + row, Instant::now());
            }
        }
        self.load_icons(cx);
        Effect::Redraw
    }

    /// Back to the top of a new list.
    fn restart(&mut self) {
        self.chosen = 0;
        self.top = 0;
        self.scroll.reset();
    }

    /// A pinned app in the grid: its icon over its name, centred.
    fn cell(&self, f: &mut Frame, n: usize, app: &App, r: Rect) {
        let c = f.theme.color;
        let id = LIST + n as HitId;
        let radius = f.px(f.theme.radius.small + 4.0);
        // Always pushed (transparent unless chosen), so choosing never shifts the list.
        let chosen = if n == self.chosen { c.accent_wash } else { Rgba::TRANSPARENT };
        f.fill(r, radius, chosen);
        f.wash(id, r, radius, true);
        let size = f.px(32.0);
        let icon = Rect::new((r.left + (r.width() - size) / 2.0).round(), r.top + f.px(10.0), size, size);
        match self.icon(&app.id) {
            Some(image) => f.push(Op::Image {
                rect: icon,
                image,
                opacity: 1.0,
                source: None,
            }),
            None => f.icon(icon, Icon::Apps, 26.0, c.label),
        }
        let label = Rect::new(
            r.left + f.px(4.0),
            icon.bottom + f.px(6.0),
            r.width() - f.px(8.0),
            f.line(TextRole::Footnote),
        );
        f.text_in(label, &app.name, TextRole::Footnote, c.label, Align::Center);
        f.focus_ring(id, r, radius);
        f.hit(id, r, Role::Button, &app.name, true, None);
    }
}

/// What a result is, for its section header: an app, a Settings page or a file.
fn kind(app: &App) -> &'static str {
    if app.id.starts_with("settings:") {
        "Settings"
    } else if app.id.starts_with("file:") {
        "Files"
    } else {
        "Apps"
    }
}

/// Each result's top in the scrolled list and the list's height (device px at `s`): a
/// section header before each kind (apps, Settings pages, files), then its rows.
fn positions(shown: &[&App], history: &History, query: &str, s: f32) -> (Vec<f32>, f32) {
    let px = |v: f32| (v * s).round();
    let (header, row) = (px(HEADER), px(36.0));
    let mut tops = Vec::with_capacity(shown.len());
    let mut y = 0.0;
    let mut category = "";
    let _ = (history, query);
    for app in shown {
        if kind(app) != category {
            category = kind(app);
            y += header;
        }
        tops.push(y);
        y += row;
    }
    (tops, y)
}

/// A section header's height inside the list, design px.
const HEADER: f32 = 26.0;

impl Panel for Launcher {
    fn width(&self) -> f32 {
        Self::WIDTH
    }

    fn view(&self, kit: &mut Kit) {
        let shown = self.shown();
        kit.frame(|f, area| {
            let c = f.theme.color;
            // The field: a well with the search glyph, the placeholder, and the text itself.
            // The field is the RichEdit's one line, centred in the well (`field_rects`).
            let field = self.field_rect;
            let well = Rect {
                left: field.left - (f.px(10.0) + f.px(18.0) + f.px(6.0)),
                right: field.right + f.px(10.0),
                top: f.px(PAD),
                bottom: f.px(PAD) + f.px(FIELD),
            };
            let radius = f.control_radius(well.height());
            f.fill(well, radius, c.well);
            let glyph = Rect {
                left: well.left + f.px(10.0),
                right: well.left + f.px(10.0) + f.px(18.0),
                ..well
            };
            f.icon(glyph, Icon::Search, 16.0, c.secondary);
            if self.query.is_empty() {
                let mut style = f.font(TextRole::Title2);
                style.weight = 400;
                let (_, h) = f.text.measure("Ag", &style);
                let y = field.top + (field.height() - h) / 2.0;
                f.label_styled(field.left + f.px(2.0), y, "Search apps", &style, c.tertiary, field.width());
            }
            if let Some(text) = &self.field {
                f.push(Op::TextField {
                    rect: field,
                    field: text.clone(),
                    revision: text.revision(),
                });
                if let Some(caret) = text.caret() {
                    f.fill(caret, 0.0, c.label);
                }
            }
            let mut column = Stack::column(Rect { top: well.bottom + f.px(10.0), ..area }, f.px(4.0));
            // The foot keeps its room: a separator and the row of buttons.
            let foot = f.px(9.0) + f.px(f.theme.metrics.control) + 2.0 * f.px(4.0);
            let cap = f.px(ROWS as f32 * 36.0 + 2.0 * HEADER);
            let room = (column.rest().height() - foot).min(cap).max(f.px(36.0));
            if self.clipboard_open {
                f.section_header(column.next(f.px(24.0)), "Clipboard history", None);
                match &self.clipboard {
                    None => {
                        let h = f.empty(column.rest(), Some(Icon::Clipboard), "Reading clipboard history…", None);
                        column.next(h);
                    }
                    Some(h) if !h.enabled => {
                        let height = f.empty(column.rest(), Some(Icon::Clipboard), "Clipboard history is off", None);
                        column.next(height);
                        super::kit::link(f, &mut column, SETTINGS, "Clipboard settings");
                    }
                    Some(h) => {
                        let row = f.px(36.0);
                        let content = h.entries.len() as f32 * row;
                        let viewport = column.next(content.min(room));
                        let width = viewport.width();
                        f.scroll(viewport, &self.scroll, Instant::now(), |f, at| {
                            let mut list = Stack::column(Rect::new(at.x, at.y, width, content), 0.0);
                            for (i, item) in h.entries.iter().enumerate() {
                                let r = ListRow {
                                    id: CLIP + i as HitId,
                                    lead: Lead::Icon(Icon::Clipboard),
                                    title: &item.text,
                                    subtitle: None,
                                    accessory: Accessory::None,
                                    selected: i == self.chosen,
                                    enabled: true,
                                };
                                f.row(&r, list.next(row));
                            }
                            content
                        });
                    }
                }
            } else if self.grid() {
                f.section_header(column.next(f.px(24.0)), "Pinned", None);
                if shown.is_empty() {
                    let h = f.empty(column.rest(), Some(Icon::Pin), "Pin apps from All apps", None);
                    column.next(h);
                } else {
                    let first = self.top / 6 * 6;
                    let count = shown.len().saturating_sub(first).min(12);
                    let (cell_h, gap) = (f.px(80.0), f.px(4.0));
                    let rows = count.div_ceil(6);
                    let grid = column.next(rows as f32 * cell_h + rows.saturating_sub(1) as f32 * gap);
                    for (n, app) in shown.iter().enumerate().skip(first).take(12) {
                        let r = layout::grid(grid, 6, gap, cell_h, n - first);
                        self.cell(f, n, app, r);
                    }
                }
                if !self.recommended.is_empty() {
                    column.space(f.px(6.0));
                    f.section_header(column.next(f.px(24.0)), "Recommended", None);
                    for (i, app) in self.recommended.iter().take(3).enumerate() {
                        let lead = self.icon(&app.id).map_or(Lead::Icon(Icon::File), Lead::Image);
                        let r = ListRow {
                            id: RECOMMENDED + i as HitId,
                            lead,
                            title: &app.name,
                            subtitle: None,
                            accessory: Accessory::None,
                            selected: false,
                            enabled: true,
                        };
                        let h = f.row_height(&r);
                        f.row(&r, column.next(h));
                    }
                }
            } else if shown.is_empty() {
                let title = if self.loading { "Finding apps…" } else { "No apps match" };
                let h = f.empty(column.rest(), Some(Icon::Search), title, None);
                column.next(h);
            } else {
                // The results: a header per kind, then rows; only this list scrolls.
                let (tops, content) = positions(&shown, &self.history, &self.query, f.s);
                let viewport = column.next(content.min(room));
                let width = viewport.width();
                let (header, row) = (f.px(HEADER), f.px(36.0));
                let recent = self.query.trim().is_empty() && !self.history.recent.is_empty();
                f.scroll(viewport, &self.scroll, Instant::now(), |f, at| {
                    let mut category = "";
                    for (i, app) in shown.iter().enumerate() {
                        let top = at.y + tops[i];
                        if kind(app) != category {
                            category = kind(app);
                            let title = if recent && category == "Apps" { "Recent and all apps" } else { category };
                            f.section_header(Rect::new(at.x, top - header, width, header), title, None);
                        }
                        let lead = match kind(app) {
                            "Settings" => Lead::Icon(Icon::Settings),
                            "Files" => self.icon(&app.id).map_or(Lead::Icon(Icon::File), Lead::Image),
                            _ => self.icon(&app.id).map_or(Lead::Icon(Icon::Apps), Lead::Image),
                        };
                        let r = ListRow {
                            id: LIST + i as HitId,
                            lead,
                            title: &app.name,
                            subtitle: None,
                            accessory: Accessory::None,
                            selected: i == self.chosen,
                            enabled: true,
                        };
                        f.row(&r, Rect::new(at.x, top, width, row));
                    }
                    content
                });
                let hint = format!("{} results  ·  ↑↓ to choose, Enter to open", shown.len());
                let line = column.next(f.line(TextRole::Footnote));
                f.text_in(line, &hint, TextRole::Footnote, c.tertiary, Align::Start);
            }
            // The foot: who is signed in, and the panel's buttons.
            let line = column.next(f.px(9.0));
            f.separator(line.left, line.right, line.top + f.px(4.0));
            let bar = column.next(f.px(f.theme.metrics.control));
            let mut rest = bar;
            for (id, icon, name) in [
                (POWER, Icon::Power, "Power"),
                (SETTINGS, Icon::Settings, "Settings"),
                (CLIPBOARD, Icon::Clipboard, "Clipboard"),
            ] {
                let (button, left) = rest.take_right(bar.height(), f.px(4.0));
                f.icon_button(id, button, icon, f.theme.metrics.control, name, false);
                rest = left;
            }
            let label = if self.all_apps { "Pinned" } else { "All apps" };
            let (w, _) = f.button_size(label, true);
            let (button, user) = rest.take_right(w, f.px(8.0));
            let icon = if self.all_apps { Icon::Pin } else { Icon::Apps };
            f.button(ALL, button, label, Some(icon), ButtonKind::Gray, true);
            if !self.user.is_empty() {
                let (badge, name) = user.take_left(bar.height(), f.px(8.0));
                f.fill(badge, badge.height() / 2.0, c.fill);
                f.icon(badge, Icon::User, 15.0, c.secondary);
                f.text_in(name, &self.user, TextRole::Callout, c.secondary, Align::Start);
            }
            column.rect.top + column.used()
        });
    }

    fn activate(&mut self, id: HitId, cx: &mut Cx) -> Effect {
        match id {
            ALL => {
                self.all_apps = !self.all_apps;
                self.clipboard_open = false;
                self.query.clear();
                if let Some(field) = &self.field {
                    let _ = field.set_text("");
                }
                self.restart();
                self.load_icons(cx);
                Effect::Redraw
            }
            SETTINGS => {
                system::open(
                    if self.clipboard_open {
                        "ms-settings:clipboard"
                    } else {
                        "ms-settings:"
                    }
                    .into(),
                );
                Effect::Close
            }
            POWER => Effect::Switch(super::Kind::Power),
            CLIPBOARD => {
                self.clipboard_open = !self.clipboard_open;
                self.restart();
                if self.clipboard_open {
                    cx.lanes.clipboard(|lane| Reply::Clipboard(lane.read()));
                }
                Effect::Redraw
            }
            id if id >= CLIP => {
                let i = (id - CLIP) as usize;
                cx.lanes.clipboard(move |lane| match lane.paste(i) {
                    Ok(()) => Reply::Done,
                    Err(e) => Reply::Failed(e),
                });
                Effect::Close
            }
            id if id >= RECOMMENDED => {
                if let Some(app) = self.recommended.get((id - RECOMMENDED) as usize) {
                    system::open(app.path.clone());
                    Effect::Close
                } else {
                    Effect::None
                }
            }
            id if id >= LIST => self.launch((id - LIST) as usize),
            _ => Effect::None,
        }
    }

    fn context(&mut self, id: HitId, _: &mut Cx) -> Effect {
        let Some(app) = id
            .checked_sub(LIST)
            .and_then(|i| self.shown().get(i as usize).copied())
            .cloned()
        else {
            return Effect::None;
        };
        if app.id.starts_with("settings:") || app.id.starts_with("file:") {
            return Effect::None;
        }
        let pin = Pinned {
            path: app.path.clone(),
            name: Some(app.name),
        };
        self.menu_entries = vec![
            if self
                .pins
                .iter()
                .any(|p| p.path.eq_ignore_ascii_case(&app.path))
            {
                Entry::action("Unpin from launcher", PanelCommand::UnpinLauncher(app.path))
            } else {
                Entry::action("Pin to launcher", PanelCommand::PinLauncher(pin.clone()))
            },
            Entry::action("Pin to dock", PanelCommand::PinDock(pin)),
        ];
        Effect::Menu
    }
    fn menu(&mut self) -> Vec<Entry<PanelCommand>> {
        std::mem::take(&mut self.menu_entries)
    }
    fn command(&mut self, command: PanelCommand, cx: &mut Cx) -> Effect {
        let previous = self.pins.clone();
        let result = mypaper_bars::config::path().and_then(|path| match command {
            PanelCommand::PinLauncher(pin) => {
                if !self
                    .pins
                    .iter()
                    .any(|p| p.path.eq_ignore_ascii_case(&pin.path))
                {
                    self.pins.push(pin);
                }
                mypaper_bars::config::set_field(&path, "launcher", "pinned", &self.pins)
            }
            PanelCommand::UnpinLauncher(path_to_remove) => {
                self.pins
                    .retain(|p| !p.path.eq_ignore_ascii_case(&path_to_remove));
                mypaper_bars::config::set_field(&path, "launcher", "pinned", &self.pins)
            }
            PanelCommand::PinDock(pin) => {
                let config = mypaper_bars::BarsConfig::load(&path)?;
                let mut pins = config.dock.pinned.unwrap_or_default();
                if !pins.iter().any(|p| p.path.eq_ignore_ascii_case(&pin.path)) {
                    pins.push(pin);
                }
                mypaper_bars::config::set_field(&path, "dock", "pinned", pins)
            }
        });
        if let Err(e) = result {
            self.pins = previous;
            log::warn!("launcher pins: {e:#}");
        }
        self.resolve_pins();
        self.refresh_pins(cx);
        self.load_icons(cx);
        Effect::Redraw
    }

    fn wheel(&mut self, _: Option<HitId>, delta: i16, cx: &mut Cx) -> Effect {
        if self.grid() {
            return self.choose(if delta > 0 { -6 } else { 6 }, cx);
        }
        if !self.scroll.wheel(delta, cx.scale, Instant::now()) {
            return Effect::None;
        }
        self.load_icons(cx);
        Effect::Redraw
    }

    fn tick(&mut self, now: Instant, cx: &mut Cx) -> (bool, Option<Instant>) {
        // The list's glide and the scrollbar's fade; icons follow what comes into view.
        let (scrolled, next) = self.scroll.tick(now);
        if scrolled {
            self.load_icons(cx);
        }
        (scrolled, next)
    }

    fn key(&mut self, key: u16, cx: &mut Cx) -> Option<Effect> {
        match key {
            k if k == windows::Win32::UI::Input::KeyboardAndMouse::VK_RIGHT.0 && self.grid() => {
                Some(self.choose(1, cx))
            }
            k if k == windows::Win32::UI::Input::KeyboardAndMouse::VK_LEFT.0 && self.grid() => {
                Some(self.choose(-1, cx))
            }
            k if k == VK_DOWN.0 => Some(self.choose(if self.grid() { 6 } else { 1 }, cx)),
            k if k == VK_UP.0 => Some(self.choose(if self.grid() { -6 } else { -1 }, cx)),
            k if k == VK_NEXT.0 => Some(self.choose(ROWS as isize, cx)),
            k if k == VK_PRIOR.0 => Some(self.choose(-(ROWS as isize), cx)),
            _ => None,
        }
    }

    fn reply(&mut self, reply: &Reply, cx: &mut Cx) -> bool {
        if let Reply::Files(files) = reply {
            let files: Vec<_> = files
                .iter()
                .map(|f| App {
                    id: format!("file:{}", f.path),
                    path: f.path.clone(),
                    name: f.name.clone(),
                    icon: f.icon.clone(),
                    keywords: vec![],
                    packaged: false,
                })
                .collect();
            // The last two apps run that are still installed (a run file or a removed app
            // isn't one), then the recent documents.
            self.recommended = self
                .history
                .recent
                .iter()
                .filter_map(|r| self.apps.iter().find(|a| a.id == r.id).cloned())
                .take(2)
                .collect();
            self.recommended.extend(files.clone());
            self.extras.retain(|a| !a.id.starts_with("file:"));
            self.extras.extend(files);
            return self.grid();
        }
        if let Reply::Clipboard(history) = reply {
            self.clipboard = Some(history.clone());
            return self.clipboard_open;
        }
        let Reply::Apps(apps) = reply else {
            return false;
        };
        let loading = std::mem::replace(&mut self.loading, false);
        if *apps == self.apps {
            return loading;
        }
        self.apps = apps.clone();
        crate::integrations::launcher_entries(&mut self.apps);
        self.resolve_pins();
        self.load_icons(cx);
        true
    }

    fn opened(&mut self, cx: &mut Cx) -> bool {
        // Load RichEdit after the first content frame; its cold DLL load must not delay the click.
        if let Some(window) = self.window.take() {
            match TextField::new(
                window,
                self.field_rect,
                false,
                Self::field_text(cx.scale),
                cx.look.theme().color.label,
            ) {
                Ok(field) => self.field = Some(field),
                Err(e) => {
                    log::warn!("launcher search field: {e:#}; opening Start");
                    mypaper_bars::system::start_menu();
                    return false;
                }
            }
        }
        self.user = index::user_name();
        self.refresh_pins(cx);
        if self.files {
            cx.lanes
                .apps(|builder| Reply::Files(builder.recent_files(60)));
        }
        self.load_icons(cx);
        true
    }

    fn refresh(&mut self, cx: &mut Cx) -> bool {
        self.grid() && self.refresh_pins(cx)
    }

    fn field(&self) -> Option<&TextField> {
        self.field.as_ref()
    }

    fn text_changed(&mut self, cx: &mut Cx) -> Effect {
        let query = self
            .field
            .as_ref()
            .and_then(|f| f.text().ok())
            .unwrap_or_default();
        let query = query.trim_end_matches(['\r', '\n']).to_owned();
        if query != self.query {
            log::debug!("launcher: query {query:?}");
            self.query = query;
            self.clipboard_open = false;
            self.restart();
            self.load_icons(cx);
        }
        // The caret or selection moved, or the text changed: the field redraws either way.
        Effect::Redraw
    }

    fn submit(&mut self, cx: &mut Cx) -> Effect {
        if self.clipboard_open {
            self.activate(CLIP + self.chosen as HitId, cx)
        } else {
            self.launch(self.chosen)
        }
    }
}

/// A shortcut path and an AppsFolder id are different launch representations of one app.
/// Reuse the indexed icon by identity; unindexed/custom pins use the same shell icon cache.
fn resolve_pin(
    pin: &Pinned,
    apps: &[App],
    described: Option<&mypaper_bars::icons::Described>,
) -> App {
    let matched = apps.iter().find(|app| {
        app.path.eq_ignore_ascii_case(&pin.path)
            || app.id.eq_ignore_ascii_case(&pin.path)
            || described
                .and_then(|d| d.aumid.as_ref())
                .is_some_and(|id| app.id.eq_ignore_ascii_case(id))
    });
    let name = pin
        .name
        .clone()
        .or_else(|| described.and_then(|d| d.name.clone()))
        .unwrap_or_else(|| mypaper_bars::apps::file_stem(&pin.path));
    let mut names = apps
        .iter()
        .filter(|app| app.name.eq_ignore_ascii_case(&name));
    let by_name = names.next().filter(|_| names.next().is_none());
    if let Some(app) = matched.or(by_name) {
        let mut app = app.clone();
        app.path = pin.path.clone();
        app.name = pin.name.clone().unwrap_or(app.name);
        app.icon = app.icon.or_else(|| described.and_then(|d| d.icon.clone()));
        return app;
    }
    App {
        id: pin.path.clone(),
        path: pin.path.clone(),
        name,
        icon: described.and_then(|d| d.icon.clone()),
        keywords: vec![],
        packaged: false,
    }
}

fn settings() -> Vec<App> {
    super::settings_pages::PAGES
        .iter()
        .map(|(name, uri, words)| App {
            id: format!("settings:{uri}"),
            name: (*name).into(),
            path: format!("ms-settings:{uri}"),
            icon: None,
            keywords: words.iter().map(|s| (*s).into()).collect(),
            packaged: false,
        })
        .collect()
}

#[cfg(test)]
mod search_tests {
    use super::*;

    fn app(name: &str) -> App {
        App {
            id: name.into(),
            name: name.into(),
            path: name.into(),
            icon: None,
            keywords: vec![],
            packaged: false,
        }
    }

    #[test]
    fn typos_and_initials_find_the_intended_app() {
        let apps = [
            "Discord",
            "Visual Studio Code",
            "Chrome",
            "Calculator",
            "Disk Cleanup",
        ]
        .map(app);
        for (query, expected) in [
            ("dipcord", "Discord"),
            ("vsc", "Visual Studio Code"),
            ("chrme", "Chrome"),
            ("dicsord", "Discord"),
        ] {
            let found = results(&apps, &History::default(), query);
            assert_eq!(
                found.first().map(|a| a.name.as_str()),
                Some(expected),
                "{query}"
            );
        }
        assert!(results(&apps, &History::default(), "xyz").is_empty());
    }

    #[test]
    fn exact_prefix_and_word_starts_beat_fuzzy_matches() {
        assert!(score("chrome", "Chrome", &[]) > score("chrome", "Chromium", &[]));
        assert!(score("cal", "Calculator", &[]) > score("cal", "Google Calendar", &[]));
        assert!(score("code", "Visual Studio Code", &[]) > score("code", "Encoder", &[]));
    }

    #[test]
    fn shortcut_pins_share_indexed_icons_and_keep_their_launch_path() {
        let mut discord = app("Discord");
        discord.id = "com.squirrel.Discord.Discord".into();
        discord.path = format!(r"shell:AppsFolder\{}", discord.id);
        discord.icon = Some("discord.png".into());
        let pin = Pinned {
            path: r"C:\TaskBar\Discord.lnk".into(),
            name: None,
        };
        let described = mypaper_bars::icons::Described {
            aumid: Some(discord.id.clone()),
            ..Default::default()
        };
        let resolved = resolve_pin(&pin, &[discord.clone()], Some(&described));
        assert_eq!(resolved.id, discord.id);
        assert_eq!(resolved.icon, discord.icon);
        assert_eq!(resolved.name, discord.name);
        assert_eq!(resolved.path, pin.path);
        let custom = resolve_pin(
            &pin,
            &[],
            Some(&mypaper_bars::icons::Described {
                icon: Some("shortcut.png".into()),
                ..Default::default()
            }),
        );
        assert_eq!(custom.icon.as_deref(), Some("shortcut.png"));
    }

    #[test]
    fn recent_and_frequent_usage_boost_only_matching_apps() {
        let apps = ["Mail", "Maps", "Discord"].map(app);
        let mut history = History::default();
        history.record("Maps", "Maps", index::epoch_ms());
        assert_eq!(results(&apps, &history, "ma")[0].name, "Maps");
        for _ in 0..30 {
            history.record("Mail", "Mail", 1);
        }
        assert_eq!(results(&apps, &history, "ma")[0].name, "Maps");
        for _ in 0..30 {
            history.record("Mail", "Mail", 1);
        }
        assert_eq!(results(&apps, &history, "ma")[0].name, "Mail");
        assert_eq!(results(&apps, &history, "dipcord")[0].name, "Discord");
        assert!(results(&apps, &history, "xyz").is_empty());
    }

    /// Run in release mode to measure the actual installed app list without a timing assertion
    /// whose outcome depends on unrelated desktop load.
    #[test]
    #[ignore]
    fn installed_app_list_search_timing() {
        let apps = index::parse_cache(
            &std::fs::read_to_string(files::local("apps.json").unwrap()).unwrap(),
        )
        .unwrap();
        let history = History::default();
        for query in ["dipcord", "vsc", "chrme"] {
            let mut samples = Vec::new();
            for _ in 0..101 {
                let start = std::time::Instant::now();
                std::hint::black_box(results(std::hint::black_box(&apps), &history, query));
                samples.push(start.elapsed());
            }
            samples.sort();
            eprintln!(
                "{} apps, {query}: median {:?}, p95 {:?}",
                apps.len(),
                samples[50],
                samples[95]
            );
        }
    }
}
