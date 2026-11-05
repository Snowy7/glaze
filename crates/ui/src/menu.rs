//! A popup menu with cascading submenus: items, layout, placement, the panel's display list,
//! and the controller that opens panels on surfaces and routes input to them.
//!
//! One topmost popup per panel: the root, and one more per open submenu level. The root is shown
//! without activating (so it paints at once), then takes the focus, so keys work and clicking
//! elsewhere closes it. Submenus never activate. A submenu butts against its parent with no gap
//! and no overlap (plan, "No popups overlap one another").

use std::cell::RefCell;
use std::sync::Arc;

use accesskit::Role;
use anyhow::Result;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_UP,
};

use crate::focus;
use crate::geom::{Point, Rect, Rgba};
use crate::icons;
use crate::render::Ui;
use crate::scene::{DisplayList, Hit, HitId, Op};
use crate::surface::{Event, Kind, Surface, SurfaceId};
use crate::text::{Text, TextStyle};
use crate::tokens::{MenuMetrics, Tokens};

thread_local! { static STYLE: RefCell<Option<Tokens>> = const { RefCell::new(None) }; }

/// The shell's shared menu style, updated when its active bars/theme change.
pub fn set_style(tokens: Option<Tokens>) {
    STYLE.with_borrow_mut(|style| *style = tokens);
}

/// One item of a menu; `C` is what choosing it does.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry<C> {
    Separator,
    Action {
        label: String,
        command: C,
        /// `None` for a plain item, `Some(on)` for a check item.
        checked: Option<bool>,
        enabled: bool,
    },
    Submenu {
        label: String,
        items: Vec<Entry<C>>,
    },
}

impl<C> Entry<C> {
    /// A plain, enabled item.
    pub fn action(label: &str, command: C) -> Self {
        Self::Action {
            label: label.into(),
            command,
            checked: None,
            enabled: true,
        }
    }

    /// An enabled check item.
    pub fn check(label: &str, command: C, on: bool) -> Self {
        Self::Action {
            label: label.into(),
            command,
            checked: Some(on),
            enabled: true,
        }
    }

    /// Can the pointer or the keyboard land on it?
    pub fn selectable(&self) -> bool {
        match self {
            Self::Separator => false,
            Self::Action { enabled, .. } => *enabled,
            Self::Submenu { .. } => true,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Separator => "",
            Self::Action { label, .. } | Self::Submenu { label, .. } => label,
        }
    }
}

/// A rectangle on screen: left, top, right, bottom.
pub type Edges = (i32, i32, i32, i32);

/// A monitor as a popup sees it: where it may go, and how large its pixels are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screen {
    pub work: Edges,
    pub dpi: u32,
}

/// The monitor at (or nearest to) a screen point.
pub fn screen_at(x: i32, y: i32) -> Screen {
    // SAFETY: plain queries with valid out pointers.
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work = if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let w = info.rcWork;
            (w.left, w.top, w.right, w.bottom)
        } else {
            (i32::MIN / 2, i32::MIN / 2, i32::MAX / 2, i32::MAX / 2)
        };
        let (mut dpi, mut unused) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused);
        Screen { work, dpi }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    pub top: f32,
    pub height: f32,
    /// The label's measured size.
    pub label: (f32, f32),
}

/// A panel's geometry in device pixels, relative to the panel's body.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    /// One per item, top to bottom.
    pub rows: Vec<Row>,
    /// The check mark's and the chevron's measured sizes.
    pub check: (f32, f32),
    pub chevron: (f32, f32),
    /// Device pixels per design pixel.
    pub scale: f32,
}

impl Layout {
    /// The body's size in whole pixels.
    pub fn size(&self) -> (i32, i32) {
        (self.width.ceil() as i32, self.height.ceil() as i32)
    }
}

/// Text styles of a menu at `scale`: labels and glyphs.
fn styles(tokens: &Tokens, scale: f32) -> (TextStyle, TextStyle) {
    let label = tokens.font.scaled(scale);
    let glyph = TextStyle::new(&tokens.icons, tokens.menu.glyph * scale);
    (label, glyph)
}

/// Measures and stacks `items` at `scale`.
pub fn layout<C>(items: &[Entry<C>], tokens: &Tokens, scale: f32, text: &mut Text) -> Layout {
    let m = tokens.menu.scaled(scale);
    let (label, glyph) = styles(tokens, scale);
    let mut top = m.pad;
    let mut widest = 0f32;
    let rows = items
        .iter()
        .map(|item| {
            let separator = matches!(item, Entry::Separator);
            let size = if separator {
                (0.0, 0.0)
            } else {
                text.measure(item.label(), &label)
            };
            let height = if separator { m.separator } else { m.row };
            let row = Row {
                top,
                height,
                label: size,
            };
            top += height;
            widest = widest.max(size.0);
            row
        })
        .collect();
    Layout {
        width: (m.pad * 2.0 + m.gutter + widest + m.gap + m.arrow).max(m.min_width),
        height: top + m.pad,
        rows,
        check: text.measure(&icons::CHECK.to_string(), &glyph),
        chevron: text.measure(&icons::CHEVRON_RIGHT.to_string(), &glyph),
        scale,
    }
}

/// Where the root menu's top-left goes: at the cursor, flipped to open leftward or upward where
/// it would run off `work`, then kept inside it.
pub fn place(size: (i32, i32), anchor: (i32, i32), work: Edges) -> (i32, i32) {
    let (w, h) = size;
    let x = if anchor.0 + w > work.2 {
        anchor.0 - w
    } else {
        anchor.0
    };
    let y = if anchor.1 + h > work.3 {
        anchor.1 - h
    } else {
        anchor.1
    };
    (
        x.clamp(work.0, (work.2 - w).max(work.0)),
        y.clamp(work.1, (work.3 - h).max(work.1)),
    )
}

/// Where a submenu goes: against the right edge of its parent (`parent` is the parent's body),
/// level with its row; against the left edge instead if the right side has no room; kept inside
/// `work` vertically. It touches the parent and never overlaps it.
pub fn place_submenu(size: (i32, i32), parent: Edges, row_top: i32, work: Edges) -> (i32, i32) {
    let (w, h) = size;
    let x = if parent.2 + w <= work.2 {
        parent.2
    } else {
        parent.0 - w
    };
    let y = row_top.min(work.3 - h).max(work.1);
    (x.max(work.0), y)
}

/// The shadow's room around the body, in whole device pixels.
fn margin(tokens: &Tokens, scale: f32) -> f32 {
    if tokens.shadow {
        (tokens.menu.shadow * scale).ceil()
    } else {
        0.0
    }
}

/// One panel as a display list in its window's pixels: the shadow margin all round, the body at
/// (margin, margin). Hit items are the rows that are not separators, with the row index as id.
pub fn view<C>(
    items: &[Entry<C>],
    layout: &Layout,
    hover: Option<usize>,
    tokens: &Tokens,
) -> DisplayList {
    let s = layout.scale;
    let m: MenuMetrics = tokens.menu.scaled(s);
    let c = &tokens.colors;
    let (label_style, glyph_style) = styles(tokens, s);
    let edge = margin(tokens, s);
    let body = Rect::new(edge, edge, layout.width, layout.height);
    let radius = (tokens.radius * s).max(0.0);
    let mut ops = Vec::with_capacity(items.len() * 2 + 3);
    if tokens.shadow {
        ops.push(Op::Shadow {
            rect: body,
            radius,
            spread: edge,
            drop: 2.0 * s,
            color: c.shadow,
        });
    }
    let tint = tokens.material.tint;
    ops.push(Op::Fill {
        rect: body,
        radius,
        color: if tokens.glass.is_some() {
            // The engine draws the glass (and its tint) under the body; this only takes clicks.
            Rgba::HIT
        } else {
            // Nearly opaque whatever the theme says: a tint has nothing behind it to blur.
            tint.with_alpha(tint.a.clamp(0.7, 1.0))
        },
    });
    if tokens.border {
        ops.push(Op::Stroke {
            rect: body.inflate(-0.5),
            radius,
            width: 1.0,
            color: c.line,
        });
    }
    let mut hits = Vec::with_capacity(items.len());
    for (index, (item, row)) in items.iter().zip(&layout.rows).enumerate() {
        let top = body.top + row.top;
        if row.top < m.pad - 0.5 || row.top + row.height > layout.height - m.pad + 0.5 {
            continue;
        }
        if let Entry::Separator = item {
            let y = top + row.height / 2.0;
            let inset = 4.0 * s;
            ops.push(Op::Fill {
                rect: Rect {
                    left: body.left + m.pad + inset,
                    top: y,
                    right: body.right - m.pad - inset,
                    bottom: y + 1.0_f32.max(s.floor()),
                },
                radius: 0.0,
                color: c.line,
            });
            continue;
        }
        let selectable = item.selectable();
        // Every row keeps its highlight op (transparent at rest), so moving the hover only
        // redraws the two rows it left and entered.
        ops.push(Op::Fill {
            rect: Rect {
                left: body.left + m.pad,
                top: top + 1.0,
                right: body.right - m.pad,
                bottom: top + row.height - 1.0,
            },
            radius: (radius * 0.6).max(2.0),
            color: if hover == Some(index) && selectable {
                c.hover
            } else {
                Rgba::TRANSPARENT
            },
        });
        let ink = if selectable { c.text } else { c.text_dim };
        let center_y = top + row.height / 2.0;
        let glyph = |at_x: f32, glyph: char, size: (f32, f32)| Op::Text {
            rect: Rect::new(at_x - size.0 / 2.0, center_y - size.1 / 2.0, size.0, size.1),
            text: glyph.to_string().into(),
            style: glyph_style.clone(),
            color: ink,
        };
        if let Entry::Action {
            checked: Some(true),
            ..
        } = item
        {
            ops.push(glyph(
                body.left + m.pad + m.gutter / 2.0,
                icons::CHECK,
                layout.check,
            ));
        }
        ops.push(Op::Text {
            rect: Rect::new(
                body.left + m.pad + m.gutter,
                center_y - row.label.1 / 2.0,
                row.label.0,
                row.label.1,
            ),
            text: item.label().into(),
            style: label_style.clone(),
            color: ink,
        });
        if let Entry::Submenu { .. } = item {
            ops.push(glyph(
                body.right - m.pad - m.arrow / 2.0,
                icons::CHEVRON_RIGHT,
                layout.chevron,
            ));
        }
        let checked = match item {
            Entry::Action { checked, .. } => *checked,
            _ => None,
        };
        hits.push(Hit {
            id: index as HitId,
            rect: Rect::new(body.left, top, body.width(), row.height),
            role: if checked.is_some() {
                Role::MenuItemCheckBox
            } else {
                Role::MenuItem
            },
            label: item.label().into(),
            enabled: selectable,
            checked,
        });
    }
    let focus = hover
        .map(|i| i as HitId)
        .filter(|id| hits.iter().any(|h| h.id == *id));
    DisplayList { ops, hits, focus }
}

/// What a menu event came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome<C> {
    /// An item was chosen; the menu is closed.
    Chosen(C),
    /// The menu closed without a choice (Escape, focus elsewhere).
    Closed,
}

/// On glass, how long a panel's content waits for the engine's glass (its usual "open to
/// glass"), and the engine's fade-in, which the content's fade follows (the wallpaper's
/// `overlay::window::FADE`). The flyouts follow the same curve.
const GLASS_DELAY: std::time::Duration = std::time::Duration::from_millis(90);
const GLASS_FADE: std::time::Duration = std::time::Duration::from_millis(300);

struct Panel<C> {
    surface: Surface,
    items: Vec<Entry<C>>,
    layout: Layout,
    natural: Layout,
    scroll: f32,
    motion: Option<crate::Tween>,
    frames: Option<crate::surface::AnimationFrames>,
    /// When it opened (on glass, its fade follows the glass from there).
    opened: std::time::Instant,
    /// The body's top-left on screen.
    origin: (i32, i32),
    hover: Option<usize>,
    /// The row of the parent panel that opened this one.
    opened_by: Option<usize>,
}

impl<C> Panel<C> {
    fn body(&self) -> Edges {
        let (w, h) = self.layout.size();
        (
            self.origin.0,
            self.origin.1,
            self.origin.0 + w,
            self.origin.1 + h,
        )
    }

    fn contains(&self, x: i32, y: i32) -> bool {
        let (l, t, r, b) = self.body();
        x >= l && y >= t && x < r && y < b
    }
}

/// The open menu, if any, and the windows it reuses.
pub struct Menu<C> {
    name: Arc<str>,
    tokens: Tokens,
    panels: Vec<Panel<C>>,
    /// Hidden windows ready for the next panel, `(surface, is_submenu)`. Creating a layered
    /// window costs about 20 ms here, more than the rest of opening a menu, so they are made
    /// ahead of time and reused.
    pool: Vec<(Surface, bool)>,
}

impl<C: Clone> Menu<C> {
    /// Creates the windows for a root panel and one submenu, hidden. `name` is what assistive
    /// technology calls the menu.
    pub fn new(name: &str) -> Result<Self> {
        let mut menu = Self {
            name: name.into(),
            tokens: Tokens::default(),
            panels: Vec::new(),
            pool: Vec::new(),
        };
        for submenu in [false, true] {
            let surface = menu.create(submenu)?;
            menu.pool.push((surface, submenu));
        }
        Ok(menu)
    }

    /// Opens `items` at `anchor` (screen pixels) on `screen`, replacing a menu already open.
    pub fn open(
        &mut self,
        ui: &mut Ui,
        items: Vec<Entry<C>>,
        anchor: (i32, i32),
        screen: Screen,
        tokens: &Tokens,
    ) -> Result<()> {
        let started = std::time::Instant::now();
        self.close();
        self.tokens = STYLE
            .with_borrow(Clone::clone)
            .unwrap_or_else(|| tokens.clone());
        // Glass when the engine can draw it: the tokens' own, else the active style's.
        self.tokens.glass = self
            .tokens
            .glass
            .filter(|_| crate::glass::available())
            .or_else(crate::glass::style);
        let scale = screen.dpi.max(96) as f32 / 96.0 * tokens.scale.clamp(0.5, 2.0);
        let opened = self.push_panel(ui, items, None, scale, |size| {
            place(size, anchor, screen.work)
        });
        if let Err(e) = opened {
            self.close();
            return Err(e);
        }
        let root = &self.panels[0].surface;
        let drawn = started.elapsed();
        root.show();
        let shown = started.elapsed();
        let how = root.take_focus();
        log::debug!(
            "menu: drawn {drawn:.1?}, shown {shown:.1?}, focused {:.1?} ({how})",
            started.elapsed()
        );
        Ok(())
    }

    /// Closes every panel. Their windows go back to the pool, hidden and without bitmaps.
    pub fn close(&mut self) {
        while self.close_innermost() {}
    }

    pub fn is_open(&self) -> bool {
        !self.panels.is_empty()
    }

    /// Is the screen point on one of the open panels?
    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.panels.iter().any(|p| p.contains(x, y))
    }

    /// Does `id` belong to one of the open panels?
    pub fn owns(&self, id: SurfaceId) -> bool {
        self.panels.iter().any(|p| p.surface.id() == id)
    }

    /// Handles an event for one of the menu's surfaces. Returns what it came to, if anything.
    pub fn handle(&mut self, ui: &mut Ui, id: SurfaceId, event: &Event) -> Option<Outcome<C>> {
        match *event {
            Event::Frame => {
                if let Some(p) = self.panels.iter_mut().find(|p| p.surface.id() == id)
                    && let Some(motion) = p.motion
                {
                    let now = std::time::Instant::now();
                    let progress = motion.value(now).clamp(0.0, 1.0);
                    let edge = margin(&self.tokens, p.layout.scale) as i32;
                    // On glass: in place, fading in on the engine's curve once its glass is
                    // there. A sliding window makes the engine re-render its glass at each
                    // step (it blinked), and content shown at once sat over the bare app
                    // behind it first (a flash).
                    let (alpha, slide, fading) = if self.tokens.glass.is_some() {
                        let since = now.saturating_duration_since(p.opened);
                        let x = (since.saturating_sub(GLASS_DELAY).as_secs_f32()
                            / GLASS_FADE.as_secs_f32())
                        .clamp(0.0, 1.0);
                        (1.0 - (1.0 - x).powi(3), 0, x < 1.0)
                    } else {
                        let slide = ((1.0 - progress) * 6.0 * p.layout.scale).round() as i32;
                        ((0.45 + progress * 0.8).min(1.0), slide, false)
                    };
                    let _ = p.surface.set_alpha((alpha * 255.0).round() as u8);
                    let _ = p
                        .surface
                        .move_to((p.origin.0 - edge, p.origin.1 - edge + slide));
                    if motion.done(now) && !fading {
                        p.motion = None;
                        p.frames = None;
                    }
                }
                None
            }
            Event::Wheel(_, delta) => {
                let panel = self.panels.iter().position(|p| p.surface.id() == id)?;
                let p = &mut self.panels[panel];
                let step =
                    self.tokens.menu.row * p.layout.scale * if delta > 0 { -3.0 } else { 3.0 };
                let scroll =
                    (p.scroll + step).clamp(0.0, (p.natural.height - p.layout.height).max(0.0));
                if scroll != p.scroll {
                    p.scroll = scroll;
                    self.redraw(ui, panel);
                }
                None
            }
            Event::Move(p) => {
                if let Some((panel, target)) = self.under(p.screen) {
                    self.select(ui, panel, target, false);
                }
                None
            }
            Event::Release(p, _) => {
                let (panel, target) = self.under(p.screen)?;
                self.select(ui, panel, target, false);
                self.choose(panel)
            }
            Event::Key(key) => self.key(ui, key),
            Event::Deactivated if self.panels.first().is_some_and(|p| p.surface.id() == id) => {
                // Focus went elsewhere: the menu is done.
                self.close();
                Some(Outcome::Closed)
            }
            Event::Invoke(hit) | Event::Focus(hit) => {
                let panel = self.panels.iter().position(|p| p.surface.id() == id)?;
                let row = hit as usize;
                let selectable = self.panels[panel].items.get(row)?.selectable();
                self.select(ui, panel, selectable.then_some(row), false);
                if matches!(event, Event::Invoke(_)) {
                    self.choose(panel)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// The innermost panel whose body holds the screen point, and the selectable row there.
    /// Over a submenu's shadow, the parent underneath answers.
    fn under(&self, (x, y): (i32, i32)) -> Option<(usize, Option<usize>)> {
        let index = self.panels.iter().rposition(|p| p.contains(x, y))?;
        let panel = &self.panels[index];
        let (ox, oy) = panel.surface.origin();
        let at = Point::new((x - ox) as f32, (y - oy) as f32);
        let row = panel
            .surface
            .shown()
            .hit_at(at)
            .filter(|h| h.enabled)
            .map(|h| h.id as usize);
        Some((index, row))
    }

    /// The command of `panel`'s selected row, if it is an action; choosing it closes the menu.
    fn choose(&mut self, panel: usize) -> Option<Outcome<C>> {
        let p = &self.panels[panel];
        let command = match p.items.get(p.hover?)? {
            Entry::Action {
                command,
                enabled: true,
                ..
            } => command.clone(),
            _ => return None,
        };
        self.close();
        Some(Outcome::Chosen(command))
    }

    /// A key press; keys always drive the innermost open panel.
    fn key(&mut self, ui: &mut Ui, key: u16) -> Option<Outcome<C>> {
        let current = self.panels.len().checked_sub(1)?;
        let hover = self.panels[current].hover;
        let on_submenu = hover
            .is_some_and(|row| matches!(self.panels[current].items[row], Entry::Submenu { .. }));
        match key {
            k if k == VK_DOWN.0 || k == VK_UP.0 => {
                let delta = if k == VK_DOWN.0 { 1 } else { -1 };
                let hits: Vec<_> = self.panels[current]
                    .items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| Hit {
                        id: i as HitId,
                        rect: Rect::default(),
                        role: Role::MenuItem,
                        label: item.label().into(),
                        enabled: item.selectable(),
                        checked: None,
                    })
                    .collect();
                let next = focus::step(&hits, hover.map(|h| h as HitId), delta);
                self.select(ui, current, next.map(|id| id as usize), false);
                None
            }
            k if k == VK_RIGHT.0 && on_submenu => {
                self.select(ui, current, hover, true);
                None
            }
            k if k == VK_LEFT.0 || k == VK_ESCAPE.0 => {
                if current > 0 {
                    self.close_innermost();
                    None
                } else {
                    self.close();
                    Some(Outcome::Closed)
                }
            }
            k if k == VK_RETURN.0 => {
                if on_submenu {
                    self.select(ui, current, hover, true);
                    None
                } else {
                    self.choose(current)
                }
            }
            _ => None,
        }
    }

    /// Makes `target` the selected row of `panel`. A submenu row opens its submenu (selecting
    /// its first item when `into` is set, for the keyboard).
    fn select(&mut self, ui: &mut Ui, panel: usize, target: Option<usize>, into: bool) {
        // A submenu of another row goes away.
        if self.panels.len() > panel + 1 && self.panels[panel + 1].opened_by != target {
            while self.panels.len() > panel + 1 {
                self.close_innermost();
            }
        }
        if self.panels[panel].hover != target {
            if let Some(row) = target {
                let p = &mut self.panels[panel];
                let r = &p.natural.rows[row];
                let pad = self.tokens.menu.pad * p.layout.scale;
                if r.top - p.scroll < pad {
                    p.scroll = r.top - pad;
                } else if r.top + r.height - p.scroll > p.layout.height - pad {
                    p.scroll = r.top + r.height - p.layout.height + pad;
                }
            }
            self.panels[panel].hover = target;
            self.redraw(ui, panel);
        }
        let Some(row) = target else { return };
        if !matches!(self.panels[panel].items[row], Entry::Submenu { .. }) {
            return;
        }
        if self.panels.len() == panel + 1 {
            self.open_submenu(ui, panel, row);
        }
        if into && let Some(sub) = self.panels.get_mut(panel + 1) {
            let first = focus::step(&sub.surface.shown().hits, None, 1);
            sub.hover = first.map(|id| id as usize);
            self.redraw(ui, panel + 1);
        }
    }

    fn open_submenu(&mut self, ui: &mut Ui, parent: usize, row: usize) {
        let Entry::Submenu { items, .. } = self.panels[parent].items[row].clone() else {
            return;
        };
        let p = &self.panels[parent];
        let body = p.body();
        let row_top = p.origin.1 + p.layout.rows[row].top.round() as i32;
        let scale = p.layout.scale;
        let work = screen_at(body.0, body.1).work;
        let opened = self.push_panel(ui, items, Some(row), scale, |size| {
            place_submenu(size, body, row_top, work)
        });
        match opened {
            Ok(()) => self.panels[parent + 1].surface.show(),
            Err(e) => log::warn!("menu: submenu: {e:#}"),
        }
    }

    /// Lays out and draws a new innermost panel (not shown yet); `place` picks its body's
    /// top-left from its size.
    fn push_panel(
        &mut self,
        ui: &mut Ui,
        items: Vec<Entry<C>>,
        opened_by: Option<usize>,
        scale: f32,
        place: impl Fn((i32, i32)) -> (i32, i32),
    ) -> Result<()> {
        let mut layout = layout(&items, &self.tokens, scale, &mut ui.text);
        let at = place(layout.size());
        let work = screen_at(at.0, at.1).work;
        let natural = layout.clone();
        layout.height = layout
            .height
            .min((work.3 - work.1) as f32 - 2.0 * margin(&self.tokens, scale));
        layout.width = layout
            .width
            .min((work.2 - work.0) as f32 - 2.0 * margin(&self.tokens, scale));
        let origin = place(layout.size());
        let surface = self.take(opened_by.is_some())?;
        self.panels.push(Panel {
            surface,
            items,
            layout,
            natural,
            scroll: 0.0,
            motion: None,
            frames: None,
            opened: std::time::Instant::now(),
            origin,
            hover: None,
            opened_by,
        });
        self.redraw_checked(ui, self.panels.len() - 1)?;
        let p = self.panels.last_mut().unwrap();
        let mut animations = windows::core::BOOL(1);
        // SAFETY: this system query writes one BOOL to our local.
        let _ = unsafe {
            windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
                windows::Win32::UI::WindowsAndMessaging::SPI_GETCLIENTAREAANIMATION,
                0,
                Some((&raw mut animations).cast()),
                Default::default(),
            )
        };
        if !animations.as_bool() {
            return Ok(());
        }
        let spring = crate::Spring::new(1.0, 420.0, 36.0);
        let glass = self.tokens.glass.is_some();
        p.motion = Some(crate::Tween::new(
            0.0,
            1.0,
            spring,
            std::time::Instant::now(),
        ));
        let mut length = spring.settle(0.001) + 0.05;
        if glass {
            length = length.max((GLASS_DELAY + GLASS_FADE).as_secs_f32() + 0.05);
        }
        p.frames = Some(
            p.surface
                .animation_frames(std::time::Duration::from_secs_f32(length)),
        );
        // On glass the content waits for its glass (see `GLASS_DELAY`); a drawn fill shows at once.
        p.surface.set_alpha(if glass { 0 } else { 115 })?;
        Ok(())
    }

    fn redraw(&mut self, ui: &mut Ui, panel: usize) {
        if let Err(e) = self.redraw_checked(ui, panel) {
            log::warn!("menu: drawing: {e:#}");
        }
    }

    fn redraw_checked(&mut self, ui: &mut Ui, panel: usize) -> Result<()> {
        let p = &mut self.panels[panel];
        for (row, natural) in p.layout.rows.iter_mut().zip(&p.natural.rows) {
            row.top = natural.top - p.scroll;
        }
        let edge = margin(&self.tokens, p.layout.scale) as i32;
        let (w, h) = p.layout.size();
        let list = view(&p.items, &p.layout, p.hover, &self.tokens);
        let at = (p.origin.0 - edge, p.origin.1 - edge);
        let size = ((w + 2 * edge) as u32, (h + 2 * edge) as u32);
        p.surface
            .set_glass(self.tokens.glass.map(|material| crate::glass::Glass {
                body: Rect::new(edge as f32, edge as f32, w as f32, h as f32),
                radius: (self.tokens.radius * p.layout.scale).max(0.0),
                // The look's blur is in DIPs; the engine takes physical px, as for flyouts.
                material: crate::GlassMaterial {
                    blur: material.blur * p.layout.scale,
                    ..material
                },
            }));
        p.surface.present(ui, at, size, list)?;
        Ok(())
    }

    /// Hides the innermost panel and keeps its window. Returns whether there was one.
    fn close_innermost(&mut self) -> bool {
        let Some(mut panel) = self.panels.pop() else {
            return false;
        };
        panel.surface.hide();
        panel.surface.release();
        // What it said before it was hidden (its own deactivation) is old news.
        panel.surface.discard_events();
        self.pool.push((panel.surface, panel.opened_by.is_some()));
        true
    }

    /// A pooled window for a root panel or a submenu, else a new one.
    fn take(&mut self, submenu: bool) -> Result<Surface> {
        match self.pool.iter().position(|(_, s)| *s == submenu) {
            Some(index) => Ok(self.pool.swap_remove(index).0),
            None => self.create(submenu),
        }
    }

    fn create(&self, submenu: bool) -> Result<Surface> {
        Surface::new_deferred_accessibility(
            Kind::Popup { activate: !submenu },
            &self.name,
            Role::Menu,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_menus_fit_the_monitor_and_keyboard_selection_scrolls_into_view() {
        let mut ui = Ui::new().unwrap();
        let mut menu = Menu::new("Long menu test").unwrap();
        menu.push_panel(
            &mut ui,
            (0..150u32)
                .map(|i| Entry::action(&format!("Theme {i}"), i))
                .collect(),
            None,
            1.0,
            |_| (100, 100),
        )
        .unwrap();
        let screen = screen_at(100, 100);
        assert!(menu.panels[0].layout.height <= (screen.work.3 - screen.work.1) as f32);
        for _ in 0..150 {
            menu.key(&mut ui, VK_DOWN.0);
        }
        let p = &menu.panels[0];
        assert_eq!(p.hover, Some(149));
        assert!(p.scroll > 0.0);
        assert!(p.surface.shown().hits.iter().any(|h| h.id == 149));
        assert!(
            p.surface
                .shown()
                .hits
                .iter()
                .all(|h| h.rect.bottom <= p.layout.height + margin(&menu.tokens, 1.0))
        );
    }

    fn items() -> Vec<Entry<u32>> {
        vec![
            Entry::Submenu {
                label: "View".into(),
                items: vec![Entry::check("Small icons", 1, true)],
            },
            Entry::action("Refresh", 2),
            Entry::Separator,
            Entry::Action {
                label: "Paste".into(),
                command: 3,
                checked: None,
                enabled: false,
            },
            Entry::action("Show more options", 4),
        ]
    }

    #[test]
    fn layout_stacks_rows_and_sizes_to_the_widest_label() {
        let mut ui = Ui::new().unwrap();
        let tokens = Tokens::default();
        let items = items();
        let one = layout(&items, &tokens, 1.0, &mut ui.text);
        let m = tokens.menu;
        assert_eq!(one.width, m.min_width, "short labels: the minimum width");
        assert_eq!(one.height, m.pad * 2.0 + 4.0 * m.row + m.separator);
        for pair in one.rows.windows(2) {
            assert_eq!(pair[0].top + pair[0].height, pair[1].top, "rows touch");
        }
        let mut wide = items.clone();
        wide.push(Entry::action(&"A very long label ".repeat(4), 5));
        assert!(layout(&wide, &tokens, 1.0, &mut ui.text).width > one.width);
        let two = layout(&items, &tokens, 2.0, &mut ui.text);
        assert_eq!(two.height, one.height * 2.0, "scale scales the panel");
    }

    #[test]
    fn the_view_hits_every_row_but_separators_and_marks_state() {
        let mut ui = Ui::new().unwrap();
        let tokens = Tokens::default();
        let items = items();
        let layout = layout(&items, &tokens, 1.0, &mut ui.text);
        let list = view(&items, &layout, Some(1), &tokens);
        let ids: Vec<HitId> = list.hits.iter().map(|h| h.id).collect();
        assert_eq!(ids, [0, 1, 3, 4], "no hit for the separator");
        assert!(!list.hits[2].enabled, "Paste is disabled");
        assert_eq!(list.focus, Some(1));
        // A point in the middle of a row hits it; the shadow margin hits nothing.
        let row = layout.rows[1];
        let edge = margin(&tokens, 1.0);
        let center = Point::new(edge + 40.0, edge + row.top + row.height / 2.0);
        assert_eq!(list.hit_at(center).map(|h| h.id), Some(1));
        assert_eq!(list.hit_at(Point::new(1.0, 1.0)), None);
        // Hover moves the highlight only: the damage stays within the two rows.
        let other = view(&items, &layout, Some(0), &tokens);
        let damage = other.damage(&list).unwrap();
        assert!(damage.top >= edge + layout.rows[0].top - 2.0);
        assert!(damage.bottom <= edge + layout.rows[2].top + 2.0);
    }

    #[test]
    fn the_menu_opens_at_the_cursor_and_flips_away_from_screen_edges() {
        let work = (0, 0, 1920, 1040);
        let size = (240, 400);
        assert_eq!(place(size, (100, 100), work), (100, 100));
        assert_eq!(place(size, (1800, 100), work), (1560, 100), "opens left");
        assert_eq!(place(size, (100, 900), work), (100, 500), "opens up");
        assert_eq!(place(size, (1800, 900), work), (1560, 500));
        // A menu taller than the screen still starts on screen.
        assert_eq!(place((240, 2000), (100, 500), work), (100, 0));
        // A second monitor to the left (negative coordinates).
        assert_eq!(place(size, (-100, 100), (-2560, 0, 0, 1440)), (-340, 100));
    }

    #[test]
    fn submenus_butt_against_their_parent_level_with_their_row() {
        let work = (0, 0, 1920, 1040);
        let size = (200, 180);
        let parent = (100, 100, 340, 600);
        assert_eq!(
            place_submenu(size, parent, 140, work),
            (340, 140),
            "no gap, no overlap"
        );
        // No room on the right: against the parent's left edge.
        let near_right = (1600, 100, 1840, 600);
        assert_eq!(place_submenu(size, near_right, 140, work), (1400, 140));
        // Low rows move up to stay on screen.
        assert_eq!(place_submenu(size, parent, 1000, work), (340, 860));
    }
}
