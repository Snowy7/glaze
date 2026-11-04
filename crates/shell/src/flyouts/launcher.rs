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
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_NEXT, VK_PRIOR, VK_TAB, VK_UP};

use super::kit::{Icon, Kit, PAD, Row, glyph};
use super::{Cx, Effect, LIST, Panel, Reply};

/// Rows shown at once.
const ROWS: usize = 8;
/// The search field's height, design px.
const FIELD: f32 = 40.0;
/// How old the app index may be before it is rebuilt in the background.
const STALE: Duration = Duration::from_secs(24 * 60 * 60);

/// How well `query` (lowercase) matches an app: higher is better, `None` for no match. A
/// prefix of the name beats a word start, which beats a substring, a keyword (the exe's name)
/// and finally the letters in order.
pub fn score(query: &str, name: &str, keywords: &[String]) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }
    let name = name.to_lowercase();
    if name.starts_with(query) {
        return Some(1000 - (name.len() - query.len()).min(200) as u32);
    }
    if name
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(query))
    {
        return Some(800);
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
        .filter_map(|app| Some((score(&query, &app.name, &app.keywords)?, recent(app), app)))
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
        let apps = cache
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|json| index::parse_cache(&json))
            .unwrap_or_default();
        if apps.is_empty() || !fresh {
            cx.lanes.apps(|builder| Reply::Apps(builder.build()));
        }
        let history_path = files::local("launcher.json").ok();
        let history = history_path
            .as_deref()
            .map(History::load)
            .unwrap_or_default();
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
        };
        launcher.load_icons(cx);
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
        }
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

    fn shown(&self) -> Vec<&App> {
        results(&self.apps, &self.history, &self.query)
    }

    /// Decodes the icons of the rows on screen (and keeps no others).
    fn load_icons(&mut self, cx: &mut Cx) {
        let wanted: Vec<(String, Option<String>)> = self
            .shown()
            .into_iter()
            .skip(self.top)
            .take(ROWS)
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
        system::open(app.path.clone());
        self.history.record(&app.id, &app.path, index::epoch_ms());
        if let Some(path) = &self.history_path {
            self.history.save(path);
        }
        Effect::Close
    }

    fn choose(&mut self, delta: isize, cx: &mut Cx) -> Effect {
        let count = self.shown().len();
        if count == 0 {
            return Effect::None;
        }
        self.chosen = (self.chosen as isize + delta).rem_euclid(count as isize) as usize;
        if self.chosen < self.top {
            self.top = self.chosen;
        } else if self.chosen >= self.top + ROWS {
            self.top = self.chosen + 1 - ROWS;
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
        if shown.is_empty() {
            kit.notice(if self.loading {
                "Finding apps…"
            } else {
                "No apps match"
            });
            return;
        }
        if self.query.is_empty() && !self.history.recent.is_empty() {
            kit.caption("Recent and all apps");
        }
        for (i, app) in shown.iter().enumerate().skip(self.top).take(ROWS) {
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

    fn activate(&mut self, id: HitId, _: &mut Cx) -> Effect {
        match id.checked_sub(LIST) {
            Some(index) => self.launch(index as usize),
            None => Effect::None,
        }
    }

    fn wheel(&mut self, _: Option<HitId>, delta: i16, cx: &mut Cx) -> Effect {
        let count = self.shown().len();
        let top = if delta > 0 {
            self.top.saturating_sub(1)
        } else {
            (self.top + 1).min(count.saturating_sub(ROWS))
        };
        if top == self.top {
            return Effect::None;
        }
        self.top = top;
        self.chosen = self.chosen.clamp(top, top + ROWS - 1);
        self.load_icons(cx);
        Effect::Redraw
    }

    fn key(&mut self, key: u16, cx: &mut Cx) -> Option<Effect> {
        match key {
            k if k == VK_DOWN.0 || k == VK_TAB.0 => Some(self.choose(1, cx)),
            k if k == VK_UP.0 => Some(self.choose(-1, cx)),
            k if k == VK_NEXT.0 => Some(self.choose(ROWS as isize, cx)),
            k if k == VK_PRIOR.0 => Some(self.choose(-(ROWS as isize), cx)),
            _ => None,
        }
    }

    fn reply(&mut self, reply: &Reply, cx: &mut Cx) -> bool {
        let Reply::Apps(apps) = reply else {
            return false;
        };
        self.loading = false;
        if let Ok(path) = files::local("apps.json") {
            let _ = files::write_atomic(&path, index::cache_json(apps).as_bytes());
        }
        if *apps == self.apps {
            return true;
        }
        self.apps = apps.clone();
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
            self.query = query;
            self.chosen = 0;
            self.top = 0;
            self.load_icons(cx);
        }
        // The caret or selection moved, or the text changed: the field redraws either way.
        Effect::Redraw
    }

    fn submit(&mut self, _: &mut Cx) -> Effect {
        self.launch(self.chosen)
    }
}
