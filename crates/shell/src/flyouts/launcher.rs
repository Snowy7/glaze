//! The launcher: a search field (windowless RichEdit, so IME, Arabic and CJK input work) over
//! the apps the Start menu lists (`mypaper_bars::flyouts::index`, cached in `apps.json`).
//! Typing filters them; Up and Down move the choice, Enter or a click runs it. With nothing
//! typed it lists what was run most recently (`launcher.json`), then the rest by name.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::Result;
use mypaper_bars::files;
use mypaper_bars::flyouts::index::{self, App, History};
use mypaper_bars::system;
use mypaper_ui::{HitId, Image, Op, Point, Rect, TextField};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_NEXT, VK_PRIOR, VK_UP};

use super::kit::{Icon, Kit, PAD, Row, glyph};
use super::{Cx, Effect, LIST, Panel, PanelCommand, Reply};
use mypaper_bars::config::{LauncherLayout, Pinned};
use mypaper_ui::Entry;

/// Rows shown at once.
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
/// and finally the letters in order.
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
    for q in query.chars() {
        loop {
            match letters.next() {
                Some(c) if c == q => break,
                Some(_) => gaps += 1,
                None => return None,
            }
        }
    }
    Some(300u32.saturating_sub(gaps.min(250)))
}

/// The apps for `query`, best first; with no query, the most recently run first.
pub fn results<'a>(apps: &'a [App], history: &History, query: &str) -> Vec<&'a App> {
    let query = query.trim().to_lowercase();
    let recent = |app: &App| {
        history
            .recent
            .iter()
            .position(|r| r.id == app.id)
            .unwrap_or(usize::MAX)
    };
    let mut scored: Vec<(u32, usize, &App)> = apps
        .iter()
        .filter_map(|app| {
            Some((
                score(&query, &app.name, &app.keywords)?
                    + history
                        .recent
                        .iter()
                        .find(|r| r.id == app.id)
                        .map_or(0, |r| r.count.min(50)),
                recent(app),
                app,
            ))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(a.1.cmp(&b.1))
            .then_with(|| a.2.name.to_lowercase().cmp(&b.2.name.to_lowercase()))
    });
    scored.into_iter().map(|(_, _, app)| app).collect()
}

pub struct Launcher {
    field: TextField,
    field_rect: Rect,
    query: String,
    apps: Vec<App>,
    history: History,
    history_path: Option<PathBuf>,
    /// The result chosen with the keyboard.
    chosen: usize,
    /// The first result shown.
    top: usize,
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

impl Launcher {
    pub fn new(cx: &mut Cx, window: HWND) -> Result<Self> {
        let s = cx.scale;
        let width = Self::WIDTH * s;
        let icon = 30.0 * s;
        let field_rect = Rect::new(
            PAD * s + icon,
            PAD * s,
            width - 2.0 * PAD * s - icon,
            FIELD * s,
        );
        let field = TextField::new(window, field_rect, false, 18.0 * s, cx.look.ink)?;
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
            field,
            field_rect,
            query: String::new(),
            loading: apps.is_empty(),
            apps,
            history,
            history_path,
            chosen: 0,
            top: 0,
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
        Ok(launcher)
    }

    const WIDTH: f32 = 560.0;

    /// A launcher over `apps` with `field` (tests), as it opens at scale 1.
    #[cfg(test)]
    pub fn with(field: TextField, apps: Vec<App>, history: History) -> Self {
        let icon = 30.0;
        Self {
            field,
            field_rect: Rect::new(PAD + icon, PAD, Self::WIDTH - 2.0 * PAD - icon, FIELD),
            query: String::new(),
            loading: false,
            apps,
            history,
            history_path: None,
            chosen: 0,
            top: 0,
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
        let icon = 30.0 * s;
        Rect::new(
            PAD * s + icon,
            PAD * s,
            (Self::WIDTH - 2.0 * PAD) * s - icon,
            FIELD * s,
        )
    }

    fn grid(&self) -> bool {
        !self.compact && !self.all_apps && self.query.trim().is_empty() && !self.clipboard_open
    }

    fn resolve_pins(&mut self) {
        self.pinned_apps = self
            .pins
            .iter()
            .map(|p| {
                self.apps
                    .iter()
                    .find(|a| {
                        a.path.eq_ignore_ascii_case(&p.path) || a.id.eq_ignore_ascii_case(&p.path)
                    })
                    .cloned()
                    .unwrap_or_else(|| App {
                        id: p.path.clone(),
                        name: p
                            .name
                            .clone()
                            .unwrap_or_else(|| mypaper_bars::apps::file_stem(&p.path)),
                        path: p.path.clone(),
                        icon: None,
                        keywords: vec![],
                        packaged: false,
                    })
            })
            .collect();
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

    /// Decodes the icons of the rows on screen (and keeps no others).
    fn load_icons(&mut self, cx: &mut Cx) {
        let wanted: Vec<(String, Option<String>)> = self
            .shown()
            .into_iter()
            .skip(self.top)
            .take(if self.grid() { 12 } else { ROWS })
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
        let rows = if self.grid() {
            12
        } else if self.clipboard_open {
            6
        } else {
            ROWS
        };
        if self.grid() {
            let row = self.chosen / 6;
            if row < self.top / 6 {
                self.top = row * 6;
            } else if row >= self.top / 6 + 2 {
                self.top = (row - 1) * 6;
            }
        } else if self.chosen < self.top {
            self.top = self.chosen;
        } else if self.chosen >= self.top + rows {
            self.top = self.chosen + 1 - rows;
        }
        self.load_icons(cx);
        Effect::Redraw
    }
}

impl Panel for Launcher {
    fn width(&self) -> f32 {
        Self::WIDTH
    }

    fn view(&self, kit: &mut Kit) {
        let r = self.field_rect;
        kit.glyph(
            Point::new(kit.left + kit.px(12.0), r.top + r.height() / 2.0),
            glyph::SEARCH,
            16.0,
            kit.look.dim(),
        );
        if self.query.is_empty() {
            let style = kit.font(18.0, 400);
            let (_, h) = kit.text.measure("Ag", &style);
            kit.text_at(
                r.left + kit.px(2.0),
                r.top + (r.height() - h) / 2.0,
                "Search apps",
                &style,
                kit.look.faint(),
                r.width(),
            );
        }
        kit.list.ops.push(Op::TextField {
            rect: r,
            field: self.field.clone(),
            revision: self.field.revision(),
        });
        if let Some(caret) = self.field.caret() {
            kit.fill(caret, 0.0, kit.look.ink);
        }
        kit.y = r.bottom + kit.px(8.0);
        kit.separator();
        let shown = self.shown();
        if self.clipboard_open {
            kit.caption("Clipboard history");
            match &self.clipboard {
                None => kit.notice("Reading clipboard history..."),
                Some(h) if !h.enabled => {
                    kit.notice("Clipboard history is off");
                    kit.footer_link(SETTINGS, "Clipboard settings");
                }
                Some(h) => {
                    for (i, item) in h.entries.iter().enumerate().skip(self.top).take(6) {
                        kit.row(Row {
                            id: CLIP + i as HitId,
                            icon: Icon::Glyph(glyph::APP),
                            title: &item.text,
                            detail: None,
                            trailing: None,
                            selected: i == self.chosen,
                            enabled: true,
                        });
                    }
                }
            }
        } else if self.grid() {
            kit.caption("Pinned");
            let cell = kit.width() / 6.0;
            let top = kit.y;
            let first = self.top / 6 * 6;
            for (n, app) in shown.iter().enumerate().skip(first).take(12) {
                let i = n - first;
                let r = Rect::new(
                    kit.left + (i % 6) as f32 * cell,
                    top + (i / 6) as f32 * kit.px(80.0),
                    cell,
                    kit.px(76.0),
                );
                let id = LIST + n as HitId;
                if n == self.chosen {
                    kit.fill(r, kit.px(8.0), kit.look.control());
                }
                kit.state(id, r, kit.px(8.0));
                if let Some(image) = self.icon(&app.id) {
                    kit.image(
                        Rect::new(
                            r.left + (cell - kit.px(28.0)) / 2.0,
                            r.top + kit.px(8.0),
                            kit.px(28.0),
                            kit.px(28.0),
                        ),
                        image,
                    );
                } else {
                    kit.glyph(
                        Point::new(r.left + cell / 2.0, r.top + kit.px(22.0)),
                        glyph::APP,
                        24.0,
                        kit.look.ink,
                    );
                }
                let style = kit.font(11.0, 400);
                kit.text_at(
                    r.left + kit.px(4.0),
                    r.top + kit.px(43.0),
                    &app.name,
                    &style,
                    kit.look.ink,
                    cell - kit.px(8.0),
                );
                kit.hit(id, r, mypaper_ui::Role::Button, &app.name, true);
            }
            kit.y =
                top + kit.px(80.0) * shown.len().saturating_sub(first).min(12).div_ceil(6) as f32;
            if shown.is_empty() {
                kit.notice("Pin apps from All apps");
            }
            kit.caption("Recommended");
            for (i, app) in self.recommended.iter().take(3).enumerate() {
                kit.row(Row {
                    id: RECOMMENDED + i as HitId,
                    icon: Icon::Glyph(glyph::APP),
                    title: &app.name,
                    detail: None,
                    trailing: None,
                    selected: false,
                    enabled: true,
                });
            }
        } else if shown.is_empty() {
            kit.notice(if self.loading {
                "Finding apps…"
            } else {
                "No apps match"
            });
        } else {
            if self.query.is_empty() && !self.history.recent.is_empty() {
                kit.caption("Recent and all apps");
            }
            let mut category = "";
            for (i, app) in shown.iter().enumerate().skip(self.top).take(ROWS) {
                let kind = if app.id.starts_with("settings:") {
                    "Settings"
                } else if app.id.starts_with("file:") {
                    "Files"
                } else {
                    "Apps"
                };
                if kind != category {
                    kit.caption(kind);
                    category = kind;
                }
                if kit.y + kit.px(82.0) > kit.limit {
                    break;
                }
                let icon = self
                    .icon(&app.id)
                    .map_or(Icon::Glyph(glyph::APP), Icon::Image);
                kit.row(Row {
                    id: LIST + i as HitId,
                    icon,
                    title: &app.name,
                    detail: None,
                    trailing: None,
                    selected: i == self.chosen,
                    enabled: true,
                });
            }
            let style = kit.font(11.0, 400);
            let hint = format!("{} apps  ·  ↑↓ to choose, Enter to open", shown.len());
            let (left, y, room) = (kit.left, kit.y + kit.px(4.0), kit.width());
            let (_, h) = kit.text_at(left, y, &hint, &style, kit.look.faint(), room);
            kit.y += h + kit.px(4.0);
        }
        kit.separator();
        if !self.user.is_empty() {
            kit.caption(&self.user);
        }
        kit.buttons(&[
            (
                ALL,
                if self.all_apps { "Pinned" } else { "All apps" },
                false,
            ),
            (CLIPBOARD, "Clipboard", false),
            (SETTINGS, "Settings", false),
            (POWER, "Power", false),
        ]);
    }

    fn activate(&mut self, id: HitId, cx: &mut Cx) -> Effect {
        match id {
            ALL => {
                self.all_apps = !self.all_apps;
                self.clipboard_open = false;
                self.query.clear();
                let _ = self.field.set_text("");
                self.chosen = 0;
                self.top = 0;
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
                self.top = 0;
                self.chosen = 0;
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
    fn command(&mut self, command: PanelCommand, _: &mut Cx) -> Effect {
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
            log::warn!("launcher pins: {e:#}");
        }
        self.resolve_pins();
        Effect::Redraw
    }

    fn wheel(&mut self, _: Option<HitId>, delta: i16, cx: &mut Cx) -> Effect {
        let count = if self.clipboard_open {
            self.clipboard.as_ref().map_or(0, |h| h.entries.len())
        } else {
            self.shown().len()
        };
        if self.grid() {
            return self.choose(if delta > 0 { -6 } else { 6 }, cx);
        }
        let rows = if self.clipboard_open { 6 } else { ROWS };
        let top = if delta > 0 {
            self.top.saturating_sub(1)
        } else {
            (self.top + 1).min(count.saturating_sub(rows))
        };
        if top == self.top {
            return Effect::None;
        }
        self.top = top;
        self.chosen = self.chosen.clamp(top, top + rows - 1);
        self.load_icons(cx);
        Effect::Redraw
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
            self.recommended = self
                .history
                .recent
                .iter()
                .take(2)
                .filter_map(|r| self.apps.iter().find(|a| a.id == r.id).cloned())
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
        self.resolve_pins();
        crate::integrations::launcher_entries(&mut self.apps);
        self.load_icons(cx);
        true
    }

    fn opened(&mut self, cx: &mut Cx) -> bool {
        self.user = index::user_name();
        if self.files {
            cx.lanes
                .apps(|builder| Reply::Files(builder.recent_files(60)));
        }
        self.load_icons(cx);
        true
    }

    fn field(&self) -> Option<&TextField> {
        Some(&self.field)
    }

    fn text_changed(&mut self, cx: &mut Cx) -> Effect {
        let query = self.field.text().unwrap_or_default();
        let query = query.trim_end_matches(['\r', '\n']).to_owned();
        if query != self.query {
            log::debug!("launcher: query {query:?}");
            self.query = query;
            self.clipboard_open = false;
            self.chosen = 0;
            self.top = 0;
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
