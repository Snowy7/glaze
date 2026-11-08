//! A scrolling strip per workspace, independent of HWNDs, clocks and Win32.
//! Mutations are immediate; callers animate from their previous rectangles to `targets()`.
#![forbid(unsafe_code)]
mod settings;
use serde::{Deserialize, Serialize};
pub use settings::{Binding, Center, Command, Direction, Settings};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WindowId(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkspaceId(pub u64);
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
impl Rect {
    pub fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Constraints {
    pub min_w: f64,
    pub min_h: f64,
    pub max_w: Option<f64>,
    pub max_h: Option<f64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}
impl Axis {
    fn length(self, r: Rect) -> f64 {
        match self {
            Self::Horizontal => r.w,
            Self::Vertical => r.h,
        }
    }
    fn breadth(self, r: Rect) -> f64 {
        match self {
            Self::Horizontal => r.h,
            Self::Vertical => r.w,
        }
    }
    fn rect(self, along: f64, across: f64, length: f64, breadth: f64) -> Rect {
        match self {
            Self::Horizontal => Rect {
                x: along,
                y: across,
                w: length,
                h: breadth,
            },
            Self::Vertical => Rect {
                x: across,
                y: along,
                w: breadth,
                h: length,
            },
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    FullWidth,
    Maximized,
    Fullscreen,
}
#[derive(Debug, Clone)]
pub struct Tile {
    pub id: WindowId,
    pub constraints: Constraints,
    pub height: f64,
}
#[derive(Debug, Clone)]
pub struct Column {
    pub tiles: Vec<Tile>,
    pub active: usize,
    pub width: f64,
    pub learned_min: f64,
    pub tabbed: bool,
    pub mode: Mode,
}
#[derive(Debug, Clone)]
pub struct Floating {
    pub id: WindowId,
    pub rect: Rect,
    pub owner: Option<WindowId>,
}
/// The previous column is an anchor, so deleting columns to its left cannot corrupt the view.
#[derive(Debug, Clone)]
struct RememberedView {
    opened: WindowId,
    previous: WindowId,
    offset: f64,
    position: Option<f64>,
}
#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub columns: Vec<Column>,
    pub floating: Vec<Floating>,
    pub focused: Option<WindowId>,
    pub view: f64,
    pub original_output: String,
    remembered: Option<RememberedView>,
    tiled_focus: Option<WindowId>,
    floating_focus: Option<WindowId>,
}
impl Workspace {
    fn empty(&self) -> bool {
        self.columns.is_empty() && self.floating.is_empty()
    }
    fn column(&self, id: WindowId) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.tiles.iter().any(|t| t.id == id))
    }
}
#[derive(Debug, Clone)]
pub struct Monitor {
    pub id: String,
    pub area: Rect,
    pub full: Rect,
    pub scale: f64,
    pub axis: Axis,
    pub workspaces: Vec<Workspace>,
    pub active: WorkspaceId,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    pub window: WindowId,
    pub workspace: WorkspaceId,
    pub rect: Rect,
    pub visible: bool,
    pub floating: bool,
}
#[derive(Debug, Clone)]
pub struct Layout {
    pub settings: Settings,
    pub monitors: Vec<Monitor>,
    pub detached: Vec<Workspace>,
    pub focused_monitor: Option<String>,
    next_workspace: u64,
    remembered_active: std::collections::HashMap<String, WorkspaceId>,
}

impl Layout {
    pub fn is_floating(&self, window: WindowId) -> bool {
        self.locate(window).is_some_and(|(mi, wi)| {
            self.monitors[mi].workspaces[wi]
                .floating
                .iter()
                .any(|f| f.id == window)
        })
    }
    /// Floating geometry is screen-fixed within its workspace and follows user/app moves.
    pub fn move_floating(&mut self, window: WindowId, rect: Rect) -> Vec<Target> {
        if let Some((mi, wi)) = self.locate(window) {
            if let Some(f) = self.monitors[mi].workspaces[wi]
                .floating
                .iter_mut()
                .find(|f| f.id == window)
            {
                f.rect = rect;
            }
        }
        self.targets()
    }
    /// A user resize or a rate-limited app width request replaces the proportional width.
    pub fn resize_column(&mut self, window: WindowId, logical_size: f64) -> Vec<Target> {
        if logical_size.is_finite() && logical_size > 0.0 {
            if let Some((mi, wi)) = self.locate(window) {
                let before = self.focus_position(mi, wi);
                let focused = self.monitors[mi].workspaces[wi].focused == Some(window);
                let m = &mut self.monitors[mi];
                let length = m.axis.length(m.area) / m.scale;
                let w = &mut m.workspaces[wi];
                if let Some(ci) = w.column(window) {
                    w.columns[ci].width = ((logical_size + self.settings.gaps)
                        / (length - self.settings.gaps).max(1.0))
                    .clamp(0.05, 1.0);
                }
                if focused {
                    self.fit(mi, wi);
                } else if let Some(before) = before {
                    self.preserve_focus(mi, wi, before);
                }
            }
        }
        self.targets()
    }
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            monitors: Vec::new(),
            detached: Vec::new(),
            focused_monitor: None,
            next_workspace: 1,
            remembered_active: std::collections::HashMap::new(),
        }
    }
    fn workspace(&mut self, output: &str) -> Workspace {
        let id = WorkspaceId(self.next_workspace);
        self.next_workspace += 1;
        Workspace {
            id,
            columns: Vec::new(),
            floating: Vec::new(),
            focused: None,
            view: 0.0,
            original_output: output.into(),
            remembered: None,
            tiled_focus: None,
            floating_focus: None,
        }
    }
    /// Update by stable output identity. Portrait outputs use a vertical strip.
    pub fn add_monitor(&mut self, id: String, area: Rect, full: Rect, scale: f64) -> Vec<Target> {
        let focused = self.focused();
        let scale = if scale.is_finite() {
            scale.clamp(0.5, 8.0)
        } else {
            1.0
        };
        if let Some(m) = self.monitors.iter_mut().find(|m| m.id == id) {
            m.area = area;
            m.full = full;
            m.scale = scale;
            m.axis = if full.h > full.w {
                Axis::Vertical
            } else {
                Axis::Horizontal
            };
        } else {
            let mut workspaces = std::mem::take(&mut self.detached);
            for m in &mut self.monitors {
                let mut i = 0;
                while i < m.workspaces.len() {
                    if m.workspaces[i].original_output == id && !m.workspaces[i].empty() {
                        workspaces.push(m.workspaces.remove(i));
                    } else {
                        i += 1;
                    }
                }
            }
            workspaces.push(self.workspace(&id));
            let active = self
                .remembered_active
                .get(&id)
                .copied()
                .filter(|id| workspaces.iter().any(|w| w.id == *id))
                .unwrap_or(workspaces[0].id);
            self.monitors.push(Monitor {
                id: id.clone(),
                area,
                full,
                scale,
                axis: if full.h > full.w {
                    Axis::Vertical
                } else {
                    Axis::Horizontal
                },
                workspaces,
                active,
            });
            if self.focused_monitor.is_none() {
                self.focused_monitor = Some(id.clone());
            }
        }
        self.normalize();
        if let Some((mi, wi)) = focused.and_then(|window| self.locate(window)) {
            self.focused_monitor = Some(self.monitors[mi].id.clone());
            self.monitors[mi].active = self.monitors[mi].workspaces[wi].id;
        }
        if let Some(mi) = self.monitors.iter().position(|m| m.id == id) {
            for wi in 0..self.monitors[mi].workspaces.len() {
                self.fit(mi, wi);
            }
        }
        self.targets()
    }
    pub fn remove_monitor(&mut self, id: &str) -> Vec<Target> {
        let mut migrated = Vec::new();
        if let Some(index) = self.monitors.iter().position(|m| m.id == id) {
            let m = self.monitors.remove(index);
            self.remembered_active.insert(m.id.clone(), m.active);
            let occupied: Vec<_> = m.workspaces.into_iter().filter(|w| !w.empty()).collect();
            migrated = occupied.iter().map(|w| w.id).collect();
            if let Some(other) = self.monitors.first_mut() {
                if self.focused_monitor.as_deref() == Some(id) && migrated.contains(&m.active) {
                    other.active = m.active;
                }
                other.workspaces.extend(occupied);
            } else {
                self.detached.extend(occupied);
            }
            if self.focused_monitor.as_deref() == Some(id) {
                self.focused_monitor = self.monitors.first().map(|m| m.id.clone());
            }
        }
        self.normalize();
        for mi in 0..self.monitors.len() {
            for wi in 0..self.monitors[mi].workspaces.len() {
                if migrated.contains(&self.monitors[mi].workspaces[wi].id) {
                    self.fit(mi, wi);
                }
            }
        }
        self.targets()
    }
    fn current(&self) -> Option<(usize, usize)> {
        let mi = self
            .monitors
            .iter()
            .position(|m| Some(&m.id) == self.focused_monitor.as_ref())?;
        Some((
            mi,
            self.monitors[mi]
                .workspaces
                .iter()
                .position(|w| w.id == self.monitors[mi].active)?,
        ))
    }
    pub fn locate(&self, id: WindowId) -> Option<(usize, usize)> {
        self.monitors.iter().enumerate().find_map(|(mi, m)| {
            m.workspaces
                .iter()
                .position(|w| w.column(id).is_some() || w.floating.iter().any(|f| f.id == id))
                .map(|wi| (mi, wi))
        })
    }
    pub fn focused(&self) -> Option<WindowId> {
        let (mi, wi) = self.current()?;
        self.monitors[mi].workspaces[wi].focused
    }
    /// Open after the owner or focus. Background opens preserve the focused tile's position.
    pub fn insert(
        &mut self,
        id: WindowId,
        constraints: Constraints,
        owner: Option<WindowId>,
        floating: Option<Rect>,
        activate: bool,
    ) -> Vec<Target> {
        if self.locate(id).is_some() {
            return self.targets();
        }
        let Some((mi, wi)) = owner
            .and_then(|o| self.locate(o))
            .or_else(|| self.current())
        else {
            return self.targets();
        };
        let output = self.monitors[mi].id.clone();
        let before = self.focus_position(mi, wi);
        let w = &mut self.monitors[mi].workspaces[wi];
        let old = w.focused;
        let old_view = w.view;
        w.original_output = output;
        if let Some(rect) = floating {
            w.floating.push(Floating { id, rect, owner });
        } else {
            let index = owner
                .or(old)
                .and_then(|id| w.column(id))
                .map_or(w.columns.len(), |i| i + 1);
            w.columns.insert(
                index,
                Column {
                    tiles: vec![Tile {
                        id,
                        constraints,
                        height: 1.0,
                    }],
                    active: 0,
                    width: self.settings.default_width,
                    learned_min: 0.0,
                    tabbed: false,
                    mode: Mode::Normal,
                },
            );
        }
        if activate || old.is_none() {
            w.focused = Some(id);
            w.remembered = old.map(|previous| RememberedView {
                opened: id,
                previous,
                offset: old_view,
                position: before,
            });
            self.monitors[mi].active = self.monitors[mi].workspaces[wi].id;
            self.focused_monitor = Some(self.monitors[mi].id.clone());
            self.fit_from(mi, wi, old);
        } else if let Some(before) = before {
            self.preserve_focus(mi, wi, before);
        }
        if activate || old.is_none() {
            let w = &mut self.monitors[mi].workspaces[wi];
            if w.column(id).is_some() {
                w.tiled_focus = Some(id);
            } else {
                w.floating_focus = Some(id);
            }
        }
        self.normalize();
        self.targets()
    }
    /// Close without shifting an unrelated focused tile; one temporary view is restored.
    pub fn remove(&mut self, id: WindowId) -> Vec<Target> {
        if let Some((mi, wi)) = self.locate(id) {
            let before = self.focus_position(mi, wi);
            let mut restored_position = None;
            let w = &mut self.monitors[mi].workspaces[wi];
            let index = w.column(id);
            if let Some(ci) = index {
                let c = &mut w.columns[ci];
                c.tiles.retain(|t| t.id != id);
                c.active = c.active.min(c.tiles.len().saturating_sub(1));
                if c.tiles.is_empty() {
                    w.columns.remove(ci);
                }
            }
            w.floating.retain(|f| f.id != id);
            if w.focused == Some(id) {
                if let Some(saved) = w.remembered.take().filter(|saved| saved.opened == id) {
                    let previous = saved.previous;
                    if w.column(previous).is_some() || w.floating.iter().any(|f| f.id == previous) {
                        w.focused = Some(previous);
                        w.view = saved.offset;
                        restored_position = saved.position;
                    } else {
                        w.focused = w
                            .columns
                            .first()
                            .map(|c| c.tiles[c.active].id)
                            .or_else(|| w.floating.last().map(|f| f.id));
                        self.fit(mi, wi);
                    }
                } else {
                    w.focused = index
                        .and_then(|i| w.columns.get(i.min(w.columns.len().saturating_sub(1))))
                        .map(|c| c.tiles[c.active].id)
                        .or_else(|| w.floating.last().map(|f| f.id))
                        .or_else(|| w.tiled_focus.filter(|id| w.column(*id).is_some()))
                        .or_else(|| w.columns.first().map(|c| c.tiles[c.active].id));
                    self.fit(mi, wi);
                }
            } else if let Some(before) = before {
                self.preserve_focus(mi, wi, before);
            }
            if let Some(position) = restored_position {
                self.preserve_focus(mi, wi, position);
            }
        } else {
            for w in &mut self.detached {
                w.columns
                    .iter_mut()
                    .for_each(|c| c.tiles.retain(|t| t.id != id));
                w.columns.retain(|c| !c.tiles.is_empty());
                for c in &mut w.columns {
                    c.active = c.active.min(c.tiles.len() - 1);
                }
                w.floating.retain(|f| f.id != id);
                if w.focused == Some(id) {
                    w.focused = w
                        .columns
                        .first()
                        .map(|c| c.tiles[0].id)
                        .or_else(|| w.floating.last().map(|f| f.id));
                }
            }
        }
        self.normalize();
        self.targets()
    }
    pub fn focus(&mut self, id: WindowId) -> Vec<Target> {
        if let Some((mi, wi)) = self.locate(id) {
            let w = &mut self.monitors[mi].workspaces[wi];
            let previous = w.focused;
            if w.focused != Some(id) {
                w.remembered = None;
            }
            w.focused = Some(id);
            if w.column(id).is_some() {
                w.tiled_focus = Some(id);
            } else {
                w.floating_focus = Some(id);
            }
            if let Some(ci) = w.column(id) {
                w.columns[ci].active = w.columns[ci]
                    .tiles
                    .iter()
                    .position(|t| t.id == id)
                    .unwrap_or(0);
            }
            self.monitors[mi].active = self.monitors[mi].workspaces[wi].id;
            self.focused_monitor = Some(self.monitors[mi].id.clone());
            self.fit_from(mi, wi, previous);
        }
        self.normalize();
        self.targets()
    }
    fn width(settings: &Settings, m: &Monitor, c: &Column) -> f64 {
        let length = m.axis.length(m.area) / m.scale;
        if matches!(c.mode, Mode::Maximized | Mode::Fullscreen) {
            return length;
        }
        let min = c
            .tiles
            .iter()
            .map(|t| match m.axis {
                Axis::Horizontal => t.constraints.min_w,
                Axis::Vertical => t.constraints.min_h,
            })
            .fold(c.learned_min, f64::max)
            .min(length);
        let max = c
            .tiles
            .iter()
            .filter_map(|t| match m.axis {
                Axis::Horizontal => t.constraints.max_w,
                Axis::Vertical => t.constraints.max_h,
            })
            .filter(|v| v.is_finite())
            .fold(length, f64::min)
            .max(min);
        let requested = if c.mode == Mode::FullWidth {
            length - settings.gaps * 2.0
        } else {
            (length - settings.gaps) * c.width - settings.gaps
        };
        requested.max(1.0).clamp(min.max(1.0), max.max(1.0))
    }
    fn start(&self, mi: usize, wi: usize, ci: usize) -> f64 {
        let m = &self.monitors[mi];
        self.settings.gaps
            + m.workspaces[wi].columns[..ci]
                .iter()
                .map(|c| Self::width(&self.settings, m, c) + self.settings.gaps)
                .sum::<f64>()
    }
    fn focus_position(&self, mi: usize, wi: usize) -> Option<f64> {
        let w = &self.monitors[mi].workspaces[wi];
        Some(self.start(mi, wi, w.column(w.focused?)?) - w.view)
    }
    fn preserve_focus(&mut self, mi: usize, wi: usize, position: f64) {
        if let Some(new) = self.focus_position(mi, wi) {
            self.monitors[mi].workspaces[wi].view += new - position;
        }
    }
    fn fit(&mut self, mi: usize, wi: usize) {
        self.fit_from(mi, wi, None);
    }
    /// On-overflow compares the target with its neighbour in the direction focus came from.
    fn fit_from(&mut self, mi: usize, wi: usize, previous: Option<WindowId>) {
        let m = &self.monitors[mi];
        let w = &m.workspaces[wi];
        let Some(ci) = w.focused.and_then(|id| w.column(id)) else {
            return;
        };
        let c = &w.columns[ci];
        let length = m.axis.length(m.area) / m.scale;
        let width = Self::width(&self.settings, m, c);
        let start = self.start(mi, wi, ci);
        let pad = self.settings.gaps.min((length - width).max(0.0) / 2.0);
        let pair_overflows = previous
            .and_then(|id| w.column(id))
            .is_some_and(|previous| {
                if previous == ci {
                    return false;
                }
                let neighbor = if previous > ci {
                    (ci + 1).min(w.columns.len() - 1)
                } else {
                    ci.saturating_sub(1)
                };
                width
                    + Self::width(&self.settings, m, &w.columns[neighbor])
                    + self.settings.gaps * 3.0
                    > length + 0.001
            });
        let view = if width >= length || matches!(c.mode, Mode::Maximized | Mode::Fullscreen) {
            start
        } else if self.settings.center_focused_column == Center::Always
            || (self.settings.center_focused_column == Center::OnOverflow && pair_overflows)
        {
            start - (length - width) / 2.0
        } else if start - pad < w.view {
            start - pad
        } else if start + width + pad > w.view + length {
            start + width + pad - length
        } else {
            w.view
        };
        self.monitors[mi].workspaces[wi].view = view;
    }
    /// A refused resize raises the learned minimum; it never shrinks automatically.
    pub fn learn_minimum(&mut self, id: WindowId, along: f64) -> Vec<Target> {
        if let Some((mi, wi)) = self.locate(id) {
            let before = self.focus_position(mi, wi);
            let focused = self.monitors[mi].workspaces[wi].focused == Some(id);
            let w = &mut self.monitors[mi].workspaces[wi];
            if let Some(ci) = w.column(id) {
                w.columns[ci].learned_min = w.columns[ci].learned_min.max(along);
            }
            if focused {
                self.fit(mi, wi);
            } else if let Some(before) = before {
                self.preserve_focus(mi, wi, before);
            }
        }
        self.targets()
    }
    /// Move a tile between overview workspaces; an insertion slot creates a fresh workspace.
    pub fn drop_on_workspace(
        &mut self,
        id: WindowId,
        monitor: &str,
        index: usize,
        new_workspace: bool,
    ) -> Vec<Target> {
        let Some(target_monitor) = self.monitors.iter().position(|m| m.id == monitor) else {
            return self.targets();
        };
        let target_workspace = self.monitors[target_monitor]
            .workspaces
            .get(index)
            .map(|w| w.id);
        let Some(saved) = self.suspend(id) else {
            return self.targets();
        };
        if let Some(mi) = self.monitors.iter().position(|m| m.id == monitor) {
            let resolved = target_workspace
                .and_then(|id| self.monitors[mi].workspaces.iter().position(|w| w.id == id));
            let index =
                resolved.unwrap_or(index.min(self.monitors[mi].workspaces.len().saturating_sub(1)));
            if new_workspace || resolved.is_none() {
                let w = self.workspace(monitor);
                self.monitors[mi].workspaces.insert(index, w);
            }
            self.monitors[mi].active = self.monitors[mi].workspaces[index].id;
            self.focused_monitor = Some(monitor.into());
            self.insert(id, saved.constraints, None, saved.floating, true);
        }
        self.normalize();
        self.targets()
    }
    /// Apply niri's focus / Ctrl move / Shift monitor grammar.
    pub fn command(&mut self, command: Command) -> Vec<Target> {
        let Some((mi, wi)) = self.current() else {
            return self.targets();
        };
        let focused = self.monitors[mi].workspaces[wi].focused;
        let ci = focused.and_then(|id| self.monitors[mi].workspaces[wi].column(id));
        match command {
            Command::Focus(d) | Command::Move(d) => {
                if let Some(ci) = ci {
                    let along = matches!(
                        (self.monitors[mi].axis, d),
                        (Axis::Horizontal, Direction::Left | Direction::Right)
                            | (Axis::Vertical, Direction::Up | Direction::Down)
                    );
                    let forward = matches!(d, Direction::Right | Direction::Down);
                    let w = &mut self.monitors[mi].workspaces[wi];
                    w.remembered = None;
                    if along {
                        let next = if forward {
                            (ci + 1).min(w.columns.len() - 1)
                        } else {
                            ci.saturating_sub(1)
                        };
                        if matches!(command, Command::Move(_)) {
                            w.columns.swap(ci, next);
                        }
                        w.focused = Some(w.columns[next].tiles[w.columns[next].active].id);
                    } else {
                        let c = &mut w.columns[ci];
                        let next = if forward {
                            (c.active + 1).min(c.tiles.len() - 1)
                        } else {
                            c.active.saturating_sub(1)
                        };
                        if matches!(command, Command::Move(_)) {
                            c.tiles.swap(c.active, next);
                        }
                        c.active = next;
                        w.focused = Some(c.tiles[next].id);
                    }
                    self.fit_from(mi, wi, focused);
                } else if let Some(id) = focused {
                    let m = &mut self.monitors[mi];
                    let area = m.area;
                    let scale = m.scale;
                    let w = &mut m.workspaces[wi];
                    if let Some(index) = w.floating.iter().position(|f| f.id == id) {
                        let origin = w.floating[index].rect;
                        if matches!(command, Command::Move(_)) {
                            let (dx, dy) = match d {
                                Direction::Left => (-50.0, 0.0),
                                Direction::Right => (50.0, 0.0),
                                Direction::Up => (0.0, -50.0),
                                Direction::Down => (0.0, 50.0),
                            };
                            let r = &mut w.floating[index].rect;
                            r.x += dx * scale;
                            r.y += dy * scale;
                            let min_x = (r.w / 4.0).clamp(10.0 * scale, 75.0 * scale);
                            let min_y = (r.h / 4.0).clamp(10.0 * scale, 75.0 * scale);
                            r.x = r.x.clamp(area.x - r.w + min_x, area.x + area.w - min_x);
                            r.y = r.y.clamp(area.y - r.h + min_y, area.y + area.h - min_y);
                        } else {
                            let (x, y) = (origin.x + origin.w / 2.0, origin.y + origin.h / 2.0);
                            let next = w
                                .floating
                                .iter()
                                .filter(|f| f.id != id)
                                .filter_map(|f| {
                                    let (dx, dy) = (
                                        f.rect.x + f.rect.w / 2.0 - x,
                                        f.rect.y + f.rect.h / 2.0 - y,
                                    );
                                    let direction = match d {
                                        Direction::Left => dx < 0.0,
                                        Direction::Right => dx > 0.0,
                                        Direction::Up => dy < 0.0,
                                        Direction::Down => dy > 0.0,
                                    };
                                    direction.then_some((f.id, dx.hypot(dy)))
                                })
                                .min_by(|(_, a), (_, b)| a.total_cmp(b))
                                .map(|(id, _)| id);
                            if let Some(next) = next {
                                w.focused = Some(next);
                                w.floating_focus = Some(next);
                            }
                        }
                    }
                }
            }
            Command::CycleWidth
            | Command::CycleHeight
            | Command::FullWidth
            | Command::Maximize
            | Command::Fullscreen
            | Command::Tabbed => {
                if let Some(ci) = ci {
                    let c = &mut self.monitors[mi].workspaces[wi].columns[ci];
                    match command {
                        Command::CycleWidth => {
                            if c.mode == Mode::FullWidth {
                                c.mode = Mode::Normal;
                            }
                            let next = self
                                .settings
                                .preset_widths
                                .iter()
                                .copied()
                                .find(|v| *v > c.width + 0.001)
                                .unwrap_or(self.settings.preset_widths[0]);
                            c.width = next;
                        }
                        Command::CycleHeight => {
                            let t = &mut c.tiles[c.active];
                            t.height = if t.height < 2.0 { t.height + 0.5 } else { 0.5 };
                        }
                        Command::FullWidth => {
                            c.mode = if c.mode == Mode::FullWidth {
                                Mode::Normal
                            } else {
                                Mode::FullWidth
                            };
                        }
                        Command::Maximize => {
                            c.mode = if c.mode == Mode::Maximized {
                                Mode::Normal
                            } else {
                                Mode::Maximized
                            }
                        }
                        Command::Fullscreen => {
                            c.mode = if c.mode == Mode::Fullscreen {
                                Mode::Normal
                            } else {
                                Mode::Fullscreen
                            }
                        }
                        Command::Tabbed => c.tabbed = !c.tabbed,
                        _ => {}
                    }
                    self.fit(mi, wi);
                }
            }
            Command::Center => {
                if let Some(ci) = ci {
                    let m = &self.monitors[mi];
                    let width = Self::width(&self.settings, m, &m.workspaces[wi].columns[ci]);
                    let view =
                        self.start(mi, wi, ci) - (m.axis.length(m.area) / m.scale - width) / 2.0;
                    self.monitors[mi].workspaces[wi].view = view;
                }
            }
            Command::Workspace(d) | Command::MoveWorkspace(d) => {
                let count = self.monitors[mi].workspaces.len();
                let next = if matches!(d, Direction::Down | Direction::Right) {
                    (wi + 1).min(count - 1)
                } else {
                    wi.saturating_sub(1)
                };
                if let (Command::MoveWorkspace(_), Some(id)) = (command, focused) {
                    let monitor = self.monitors[mi].id.clone();
                    return self.drop_on_workspace(id, &monitor, next, false);
                }
                self.monitors[mi].active = self.monitors[mi].workspaces[next].id;
            }
            Command::FocusMonitor(d) | Command::MoveMonitor(d) => {
                let from = self.monitors[mi].full;
                let next = self
                    .monitors
                    .iter()
                    .enumerate()
                    .filter(|(i, m)| {
                        *i != mi
                            && match d {
                                Direction::Left => m.full.x < from.x,
                                Direction::Right => m.full.x > from.x,
                                Direction::Up => m.full.y < from.y,
                                Direction::Down => m.full.y > from.y,
                            }
                    })
                    .min_by(|(_, a), (_, b)| {
                        ((a.full.x - from.x).hypot(a.full.y - from.y))
                            .total_cmp(&((b.full.x - from.x).hypot(b.full.y - from.y)))
                    })
                    .map(|(i, _)| i);
                if let Some(next) = next {
                    let monitor = self.monitors[next].id.clone();
                    if let (Command::MoveMonitor(_), Some(id)) = (command, focused) {
                        let index = self.monitors[next]
                            .workspaces
                            .iter()
                            .position(|w| w.id == self.monitors[next].active)
                            .unwrap_or(0);
                        return self.drop_on_workspace(id, &monitor, index, false);
                    }
                    self.focused_monitor = Some(monitor);
                }
            }
            Command::Consume => {
                if let Some(ci) = ci.filter(|ci| *ci > 0) {
                    let w = &mut self.monitors[mi].workspaces[wi];
                    let active = w.columns[ci].active;
                    let tile = w.columns[ci].tiles.remove(active);
                    w.columns[ci].active = w.columns[ci]
                        .active
                        .min(w.columns[ci].tiles.len().saturating_sub(1));
                    w.columns[ci - 1].tiles.push(tile);
                    w.columns[ci - 1].active = w.columns[ci - 1].tiles.len() - 1;
                    if w.columns[ci].tiles.is_empty() {
                        w.columns.remove(ci);
                    }
                    self.fit(mi, wi);
                }
            }
            Command::Expel => {
                if let Some(ci) = ci {
                    let w = &mut self.monitors[mi].workspaces[wi];
                    if w.columns[ci].tiles.len() > 1 {
                        let c = &mut w.columns[ci];
                        let tile = c.tiles.remove(c.active);
                        c.active = c.active.min(c.tiles.len() - 1);
                        let width = c.width;
                        w.columns.insert(
                            ci + 1,
                            Column {
                                tiles: vec![tile],
                                active: 0,
                                width,
                                learned_min: 0.0,
                                tabbed: false,
                                mode: Mode::Normal,
                            },
                        );
                        self.fit(mi, wi);
                    }
                }
            }
            Command::Floating => {
                if let Some(id) = focused {
                    if let Some(ci) = ci {
                        let rect = self
                            .targets()
                            .into_iter()
                            .find(|t| t.window == id)
                            .map(|t| t.rect)
                            .unwrap_or_default();
                        let constraints = self.monitors[mi].workspaces[wi].columns[ci]
                            .tiles
                            .iter()
                            .find(|t| t.id == id)
                            .unwrap()
                            .constraints;
                        self.remove(id);
                        return self.insert(id, constraints, None, Some(rect), true);
                    } else {
                        self.remove(id);
                        return self.insert(id, Constraints::default(), None, None, true);
                    }
                }
            }
            Command::Overview => {}
            Command::FocusLayer => {
                let w = &mut self.monitors[mi].workspaces[wi];
                let next = if focused.is_some_and(|id| w.column(id).is_some()) {
                    w.floating_focus
                        .filter(|id| w.floating.iter().any(|f| f.id == *id))
                        .or_else(|| w.floating.last().map(|f| f.id))
                } else {
                    w.tiled_focus
                        .filter(|id| w.column(*id).is_some())
                        .or_else(|| w.columns.first().map(|c| c.tiles[c.active].id))
                };
                if let Some(id) = next {
                    self.focus(id);
                }
            }
        }
        self.normalize();
        self.targets()
    }
    fn normalize(&mut self) {
        for mi in 0..self.monitors.len() {
            let m = &mut self.monitors[mi];
            let active = m.active;
            m.workspaces.retain(|w| !w.empty() || w.id == active);
            if !m.workspaces.iter().any(|w| w.id == m.active) {
                if let Some(w) = m.workspaces.first() {
                    m.active = w.id;
                }
            }
            if m.workspaces.last().is_none_or(|w| !w.empty()) {
                let output = m.id.clone();
                let w = self.workspace(&output);
                self.monitors[mi].workspaces.push(w);
            }
            if !self.monitors[mi]
                .workspaces
                .iter()
                .any(|w| w.id == self.monitors[mi].active)
            {
                self.monitors[mi].active = self.monitors[mi].workspaces[0].id;
            }
        }
    }
    /// Target rectangles in physical screen coordinates. Invisible targets must be parked by the host.
    pub fn targets(&self) -> Vec<Target> {
        let mut result = Vec::new();
        for m in &self.monitors {
            for w in &m.workspaces {
                let mut along = self.settings.gaps - w.view;
                for c in &w.columns {
                    let width = Self::width(&self.settings, m, c);
                    let area = if c.mode == Mode::Fullscreen {
                        m.full
                    } else {
                        m.area
                    };
                    let cross_gap = if matches!(c.mode, Mode::Normal | Mode::FullWidth) {
                        self.settings.gaps
                    } else {
                        0.0
                    };
                    let breadth = (m.axis.breadth(area) / m.scale - cross_gap * 2.0).max(1.0);
                    let heights = heights(c, m.axis, breadth, self.settings.gaps);
                    let mut across = cross_gap;
                    for (ti, t) in c.tiles.iter().enumerate() {
                        let (a, b, len, br) = if c.mode == Mode::Fullscreen {
                            (along, 0.0, m.axis.length(area) / m.scale, breadth)
                        } else {
                            (along, across, width, heights[ti])
                        };
                        let mut r = m.axis.rect(a, b, len, br);
                        r.x = area.x + r.x * m.scale;
                        r.y = area.y + r.y * m.scale;
                        r.w *= m.scale;
                        r.h *= m.scale;
                        // Whole columns only: no window may bleed onto an adjacent output.
                        let visible = w.id == m.active
                            && (!(c.tabbed || c.mode == Mode::Fullscreen) || ti == c.active)
                            && r.x >= area.x - 0.5
                            && r.y >= area.y - 0.5
                            && r.x + r.w <= area.x + area.w + 0.5
                            && r.y + r.h <= area.y + area.h + 0.5;
                        result.push(Target {
                            window: t.id,
                            workspace: w.id,
                            rect: r,
                            visible,
                            floating: false,
                        });
                        if !c.tabbed {
                            across += heights[ti] + self.settings.gaps;
                        }
                    }
                    along += width + self.settings.gaps;
                }
                for f in &w.floating {
                    result.push(Target {
                        window: f.id,
                        workspace: w.id,
                        rect: f.rect,
                        visible: w.id == m.active,
                        floating: true,
                    });
                }
            }
        }
        result
    }
}

/// A native fullscreen window keeps enough state to return to its original column.
#[derive(Debug, Clone)]
pub struct Suspended {
    window: WindowId,
    monitor: String,
    workspace: WorkspaceId,
    column: usize,
    width: f64,
    constraints: Constraints,
    floating: Option<Rect>,
    neighbor: Option<WindowId>,
    tile_index: usize,
    height: f64,
    tabbed: bool,
    mode: Mode,
    learned_min: f64,
}
impl Layout {
    /// Untile a fixed-size or incompatible client while preserving its workspace.
    pub fn float_at(&mut self, window: WindowId, rect: Rect) -> Vec<Target> {
        if let Some(mut saved) = self.suspend(window) {
            saved.floating = Some(rect);
            self.resume(saved, false)
        } else {
            self.targets()
        }
    }
    pub fn suspend(&mut self, window: WindowId) -> Option<Suspended> {
        let (mi, wi) = self.locate(window)?;
        let m = &self.monitors[mi];
        let w = &m.workspaces[wi];
        let column = w.column(window);
        let source = column.map(|ci| &w.columns[ci]);
        let saved = Suspended {
            window,
            monitor: m.id.clone(),
            workspace: w.id,
            column: column.unwrap_or(0),
            width: column.map_or(self.settings.default_width, |i| w.columns[i].width),
            constraints: column
                .and_then(|i| w.columns[i].tiles.iter().find(|t| t.id == window))
                .map_or(Constraints::default(), |t| t.constraints),
            floating: w.floating.iter().find(|f| f.id == window).map(|f| f.rect),
            neighbor: source
                .and_then(|c| c.tiles.iter().find(|t| t.id != window))
                .map(|t| t.id),
            tile_index: source
                .and_then(|c| c.tiles.iter().position(|t| t.id == window))
                .unwrap_or(0),
            height: source
                .and_then(|c| c.tiles.iter().find(|t| t.id == window))
                .map_or(1.0, |t| t.height),
            tabbed: source.is_some_and(|c| c.tabbed),
            mode: source.map_or(Mode::Normal, |c| c.mode),
            learned_min: source.map_or(0.0, |c| c.learned_min),
        };
        self.remove(window);
        Some(saved)
    }
    pub fn resume(&mut self, saved: Suspended, activate: bool) -> Vec<Target> {
        let Some(mi) = self
            .monitors
            .iter()
            .position(|m| m.id == saved.monitor)
            .or_else(|| (!self.monitors.is_empty()).then_some(0))
        else {
            return self.targets();
        };
        let wi = self.monitors[mi]
            .workspaces
            .iter()
            .position(|w| w.id == saved.workspace)
            .unwrap_or(0);
        let previous = self.focused_monitor.clone();
        let active = self.monitors[mi].active;
        self.focused_monitor = Some(self.monitors[mi].id.clone());
        self.monitors[mi].active = self.monitors[mi].workspaces[wi].id;
        self.insert(
            saved.window,
            saved.constraints,
            None,
            saved.floating,
            activate,
        );
        if let Some((mi, wi)) = self.locate(saved.window) {
            let w = &mut self.monitors[mi].workspaces[wi];
            if let Some(ci) = w.column(saved.window) {
                let mut c = w.columns.remove(ci);
                if let Some(neighbor) = saved.neighbor.and_then(|id| w.column(id)) {
                    let tile = c.tiles.pop().unwrap();
                    let c = &mut w.columns[neighbor];
                    let active = c.tiles[c.active].id;
                    let index = saved.tile_index.min(c.tiles.len());
                    c.tiles.insert(
                        index,
                        Tile {
                            height: saved.height,
                            ..tile
                        },
                    );
                    c.active = if activate {
                        index
                    } else {
                        c.tiles.iter().position(|t| t.id == active).unwrap()
                    };
                } else {
                    c.width = saved.width;
                    c.tabbed = saved.tabbed;
                    c.mode = saved.mode;
                    c.learned_min = saved.learned_min;
                    c.tiles[0].height = saved.height;
                    w.columns.insert(saved.column.min(w.columns.len()), c);
                }
            }
            self.fit(mi, wi);
        }
        if !activate {
            self.focused_monitor = previous;
            self.monitors[mi].active = active;
        }
        self.normalize();
        self.targets()
    }
    /// Native maximize/restore requests are idempotent, unlike a keyboard toggle.
    pub fn set_maximized(&mut self, window: WindowId, on: bool) -> Vec<Target> {
        if let Some((mi, wi)) = self.locate(window) {
            let w = &mut self.monitors[mi].workspaces[wi];
            if let Some(ci) = w.column(window) {
                w.columns[ci].mode = if on { Mode::Maximized } else { Mode::Normal };
            }
            self.fit(mi, wi);
        }
        self.targets()
    }
    /// A title-bar drop near either column edge inserts; its centre joins the stack.
    pub fn drop_at(&mut self, window: WindowId, x: f64, y: f64) -> Vec<Target> {
        let target = self
            .targets()
            .into_iter()
            .find(|t| t.window != window && t.visible && !t.floating && t.rect.contains(x, y));
        let Some(target) = target else {
            return self.targets();
        };
        let Some(saved) = self.suspend(window) else {
            return self.targets();
        };
        let Some((mi, wi)) = self.locate(target.window) else {
            return self.resume(saved, true);
        };
        let m = &mut self.monitors[mi];
        let axis = m.axis;
        let w = &mut m.workspaces[wi];
        let ci = w.column(target.window).unwrap();
        let fraction = match axis {
            Axis::Horizontal => (x - target.rect.x) / target.rect.w,
            Axis::Vertical => (y - target.rect.y) / target.rect.h,
        };
        let tile = Tile {
            id: window,
            constraints: saved.constraints,
            height: 1.0,
        };
        if !(0.3..=0.7).contains(&fraction) {
            let index = ci + usize::from(fraction > 0.7);
            w.columns.insert(
                index,
                Column {
                    tiles: vec![tile],
                    active: 0,
                    width: saved.width,
                    learned_min: 0.0,
                    tabbed: false,
                    mode: Mode::Normal,
                },
            );
        } else {
            let c = &mut w.columns[ci];
            c.tiles.push(tile);
            c.active = c.tiles.len() - 1;
        }
        w.focused = Some(window);
        m.active = w.id;
        self.focused_monitor = Some(m.id.clone());
        self.fit(mi, wi);
        self.normalize();
        self.targets()
    }
}

/// Weighted auto heights honor every minimum, even when that overflows the work area.
fn heights(c: &Column, axis: Axis, breadth: f64, gap: f64) -> Vec<f64> {
    if c.tabbed {
        return vec![breadth; c.tiles.len()];
    }
    let mut remaining = (breadth - gap * c.tiles.len().saturating_sub(1) as f64).max(1.0);
    let mut result = vec![0.0; c.tiles.len()];
    let mut pending: Vec<usize> = (0..c.tiles.len()).collect();
    loop {
        let weight: f64 = pending.iter().map(|&i| c.tiles[i].height).sum();
        let mut fixed = false;
        pending.retain(|&i| {
            let t = &c.tiles[i];
            let min = match axis {
                Axis::Horizontal => t.constraints.min_h,
                Axis::Vertical => t.constraints.min_w,
            }
            .max(1.0);
            let max = match axis {
                Axis::Horizontal => t.constraints.max_h,
                Axis::Vertical => t.constraints.max_w,
            };
            if min > remaining * t.height / weight || max == Some(min) {
                result[i] = min;
                remaining -= min;
                fixed = true;
                false
            } else {
                true
            }
        });
        if pending.is_empty() {
            break;
        }
        if !fixed {
            let weight: f64 = pending.iter().map(|&i| c.tiles[i].height).sum();
            for &i in &pending {
                result[i] = (remaining * c.tiles[i].height / weight).max(1.0);
            }
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests;
