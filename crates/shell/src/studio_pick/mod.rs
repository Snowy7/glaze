//! Transient desktop picking while Studio's panel is open. Selection never writes config.
//! Catchers sit below widgets in the desktop band; widget input still reaches its real HWND.
mod chrome;
mod notice;
pub mod preview;
pub mod resize;
pub mod wire;

use anyhow::Result;
use mypaper_bars::config::BarsConfig;
use mypaper_desktop::{DesktopHost, Monitor, monitor_infos};
use mypaper_ui::{
    Button, DisplayList, Event, Kind as SurfaceKind, Op, Point, Rect, Rgba, Role, Surface,
    SurfaceId, TextStyle, Ui,
};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::IsWindow;
use wire::{Kind, Message, Selection};

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub selection: Selection,
    pub label: String,
    pub rect: Rect,
    /// Physical-pixel radius of the visible surface, shared by frame and corner handles.
    pub radius: f32,
    pub topmost: bool,
    pub surface: SurfaceId,
    /// Modules are selected by their chip; the body's normal action still runs.
    pub body_selects: bool,
    pub resize: Option<resize::Spec>,
}

#[derive(Default, Debug)]
struct Session {
    owner: Option<(isize, u32)>,
    selected: Option<Selection>,
    highlight: Option<String>,
}

impl Session {
    fn begin(&mut self, owner: isize, pid: u32) {
        if self.owner != Some((owner, pid)) {
            *self = Self {
                owner: Some((owner, pid)),
                ..Default::default()
            };
        }
    }
    fn end(&mut self, owner: isize) {
        if self.owner.is_some_and(|o| o.0 == owner) {
            *self = Self::default();
        }
    }
    fn check(&mut self, alive: impl FnOnce(isize, u32) -> bool) {
        if self.owner.is_some_and(|(h, p)| !alive(h, p)) {
            *self = Self::default();
        }
    }
    fn active(&self) -> bool {
        self.owner.is_some()
    }
    fn lifted(&self) -> BTreeSet<String> {
        self.highlight
            .iter()
            .cloned()
            .chain(
                self.selected
                    .iter()
                    .filter(|s| s.kind == Kind::Widget)
                    .map(|s| s.id.clone()),
            )
            .collect()
    }
}

struct Catcher {
    surface: Surface,
    display: String,
}
struct Decoration {
    frame: Surface,
    chip: Surface,
    chip_size: (f32, f32),
    handles: Vec<(Surface, resize::Corner)>,
    target: Target,
    selected: bool,
}

struct Gesture {
    selection: Selection,
    spec: resize::Spec,
    corner: Option<resize::Corner>,
    start: Point,
    pending: ([u32; 2], [u32; 2]),
    capture: SurfaceId,
    window: HWND,
    _frames: mypaper_ui::AnimationFrames,
}

impl Gesture {
    fn new(target: &Target, surface: &Surface, point: Point, corner: Option<resize::Corner>) -> Option<Self> {
        let spec = target.resize.as_ref()?;
        // SAFETY: this is a shell-owned no-activate widget, chip or corner on the UI thread.
        unsafe { windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(surface.hwnd()); }
        Some(Self {
            selection: target.selection.clone(), spec: spec.clone(), corner, start: point,
            pending: (spec.cell, spec.size), capture: surface.id(), window: surface.hwnd(),
            _frames: surface.animation_frames(Duration::from_secs(3600)),
        })
    }

    fn update(&mut self, point: Point) {
        let delta = Point::new(point.x - self.start.x, point.y - self.start.y);
        self.pending = match self.corner {
            Some(corner) => self.spec.target(corner, delta),
            None => self.spec.moved(delta),
        };
    }
}

impl Drop for Gesture {
    fn drop(&mut self) {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture};
        // SAFETY: release only this gesture's capture on its owning UI thread.
        unsafe {
            if GetCapture() == self.window {
                let _ = ReleaseCapture();
            }
        }
    }
}

pub struct StudioPick {
    endpoint: wire::Endpoint,
    session: Session,
    panel: HWND,
    catchers: Vec<Catcher>,
    decorations: Vec<Decoration>,
    hovered: Option<Target>,
    hover_until: Option<Instant>,
    monitors: Vec<Monitor>,
    anchor: Option<HWND>,
    pressed: bool,
    geometry_due: Instant,
    notice: Option<notice::Notice>,
    gesture: Option<Gesture>,
    preview: preview::Preview,
    resize_preview: Option<resize::Preview>,
    widget_revision: Option<u64>,
    warm_preview: bool,
}

impl StudioPick {
    pub fn new() -> Result<Self> {
        Ok(Self {
            endpoint: wire::Endpoint::new(true)?,
            session: Session::default(),
            panel: HWND::default(),
            catchers: Vec::new(),
            decorations: Vec::new(),
            hovered: None,
            hover_until: None,
            monitors: Vec::new(),
            anchor: None,
            pressed: false,
            geometry_due: Instant::now(),
            notice: None,
            gesture: None,
            preview: preview::Preview::default(),
            resize_preview: None,
            widget_revision: None,
            warm_preview: false,
        })
    }
    pub fn timeout_ms(&self) -> u32 {
        if self.session.active() { 50 } else { u32::MAX }
    }
    pub fn active(&self) -> bool {
        self.session.active()
    }

    pub fn take_preview_warm(&mut self) -> bool {
        std::mem::take(&mut self.warm_preview)
    }

    pub fn preview(&mut self) -> &mut preview::Preview { &mut self.preview }

    pub fn previews(&mut self) -> (&mut preview::Preview, Option<&resize::Preview>) {
        (&mut self.preview, self.resize_preview.as_ref())
    }

    pub fn lifted(&self) -> BTreeSet<String> {
        // Keep the receiving HWND alive until its button release has run.
        if self.pressed && self.gesture.is_none() {
            self.session.highlight.iter().cloned().collect()
        } else {
            self.session.lifted()
        }
    }

    pub fn tick(&mut self, ui: &mut Ui, targets: &[Target], bars: &BarsConfig, widget_revision: Option<u64>) {
        self.widget_revision = widget_revision;
        let mut previous_owner = self.session.owner;
        while let Some((source, request)) = self.endpoint.next() {
            match request {
                Message::Begin { panel } => {
                    let pid = wire::process(source);
                    let panel = HWND(panel as _);
                    if pid != 0 && wire::process(panel) == pid {
                        self.session.begin(source.0 as isize, pid);
                        if self.session.owner != previous_owner {
                            self.reset_transients();
                            previous_owner = self.session.owner;
                        }
                        self.panel = panel;
                    }
                }
                Message::End => self.session.end(source.0 as isize),
                Message::WarmPreview if self.owns(source) => self.warm_preview = true,
                Message::Preview { session, sequence, documents }
                    if self.owns(source) => self.preview.update(session, sequence, documents),
                Message::PreviewCommit { session, sequence, revisions }
                    if self.owns(source) => self.preview.saved(session, sequence, &revisions),
                Message::PreviewDock {
                    session,
                    sequence,
                    base_revision,
                    size,
                } if self.session.owner.is_some_and(|(h, pid)| {
                    h == source.0 as isize && wire::process(source) == pid
                }) =>
                {
                    self.preview.dock(session, sequence, base_revision, size);
                }
                Message::PreviewSaved {
                    session,
                    sequence,
                    revision,
                } if self.session.owner.is_some_and(|(h, pid)| {
                    h == source.0 as isize && wire::process(source) == pid
                }) =>
                {
                    self.preview.saved(session, sequence, &[wire::Revision { document: wire::Document::Bars, revision }]);
                }
                Message::PreviewCancel { session, sequence }
                    if self.session.owner.is_some_and(|(h, pid)| {
                        h == source.0 as isize && wire::process(source) == pid
                    }) =>
                {
                    self.preview.cancel(session, sequence);
                }
                Message::UndoStep { display, text }
                    if self.session.owner.is_some_and(|(h, pid)| {
                        h == source.0 as isize && wire::process(source) == pid
                    }) && text.chars().count() <= 200
                        && !text.chars().any(char::is_control) =>
                {
                    if let Some(notice) = &mut self.notice {
                        notice.update(display, text, 0);
                    } else {
                        self.notice = Some(notice::Notice::new(display, text, 0));
                    }
                }
                Message::UndoNotice { display, text, step }
                    if self.owns(source) && text.chars().count() <= 200 && !text.chars().any(char::is_control) => {
                    if let Some(notice) = &mut self.notice { notice.update(display, text, step); }
                    else { self.notice = Some(notice::Notice::new(display, text, step)); }
                }
                Message::UndoDismiss if self.owns(source) => self.notice = None,
                Message::ResizeResult { selection, cell, size, revision } if self.owns(source) => {
                    if let Some(preview) = &mut self.resize_preview && preview.selection == selection
                        && preview.cell == cell && preview.size == size {
                        if let Some(revision) = revision { preview.saved_revision = Some(revision); }
                        else { self.resize_preview = None; }
                    }
                }
                Message::ClearSelection
                    if self.session.owner.is_some_and(|(h, pid)| {
                        h == source.0 as isize && wire::process(source) == pid
                    }) =>
                {
                    self.gesture = None;
                    self.resize_preview = None;
                    self.session.selected = None;
                    self.session.highlight = None;
                    self.hovered = None;
                    self.hover_until = None;
                }
                Message::Highlight { id }
                    if self.session.owner.is_some_and(|(h, pid)| {
                        h == source.0 as isize && wire::process(source) == pid
                    }) =>
                {
                    self.session.highlight = id;
                }
                Message::Selected { selection } if self.owns(source) => {
                    if self.session.selected.as_ref() != Some(&selection) {
                        self.gesture = None;
                        self.resize_preview = None;
                    }
                    self.session.selected = Some(selection);
                }
                _ => {}
            }
        }
        let panel = self.panel;
        self.session
            .check(|h, p| wire::process(HWND(h as _)) == p && wire::process(panel) == p);
        if self.session.owner != previous_owner {
            self.reset_transients();
        }
        if self.resize_preview.as_ref().is_some_and(|p| !widget_revision.is_some_and(|r| p.current(r))) {
            self.resize_preview = None;
        }
        if !self.session.active() {
            self.gesture = None;
            self.catchers.clear();
            self.decorations.clear();
            self.hovered = None;
            self.hover_until = None;
            self.monitors.clear();
            self.anchor = None;
            self.pressed = false;
            return;
        }
        // Resolve Explorer only when its generation changes; find() asks it to create WorkerW.
        // SAFETY: read-only validity query of the cached desktop anchor.
        let anchor_alive = self
            .anchor
            .is_some_and(|h| unsafe { IsWindow(Some(h)).as_bool() });
        let anchor = if anchor_alive {
            self.anchor
        } else {
            DesktopHost::find().ok().map(|h| h.icons_parent())
        };
        let refresh =
            self.catchers.is_empty() || !anchor_alive || self.geometry_due <= Instant::now();
        let monitors = if refresh {
            monitor_infos()
        } else {
            self.monitors.clone()
        };
        if refresh {
            self.geometry_due = Instant::now() + Duration::from_secs(1);
        }
        let geometry: Vec<_> = monitors
            .iter()
            .map(|m| (&m.device, m.rect, m.work, m.dpi))
            .collect();
        let previous: Vec<_> = self
            .monitors
            .iter()
            .map(|m| (&m.device, m.rect, m.work, m.dpi))
            .collect();
        if self.catchers.is_empty() || anchor != self.anchor || geometry != previous {
            self.gesture = None;
            self.catchers.clear();
            self.decorations.clear();
            self.monitors = monitors;
            self.anchor = anchor;
            if let Some(parent) = self.anchor {
                let icons = mypaper_desktop::desktop_view::DesktopView::open()
                    .and_then(|v| v.icon_rects())
                    .unwrap_or_default();
                for monitor in &self.monitors {
                    let result = (|| -> Result<Catcher> {
                        let mut surface = Surface::new(
                            SurfaceKind::DesktopChild { parent },
                            "Studio wallpaper picker",
                            Role::Pane,
                        )?;
                        surface.set_no_activate(true);
                        let r = monitor.rect;
                        let full = Rect::new(0.0, 0.0, r.width as f32, r.height as f32);
                        let holes: Vec<_> = icons
                            .iter()
                            .map(|i| {
                                Rect::new(
                                    (i.x - r.x) as f32,
                                    (i.y - r.y) as f32,
                                    i.width as f32,
                                    i.height as f32,
                                )
                            })
                            .collect();
                        let list = DisplayList {
                            ops: subtract(full, &holes)
                                .into_iter()
                                .map(|rect| Op::Fill {
                                    rect,
                                    radius: 0.0,
                                    color: Rgba::HIT,
                                })
                                .collect(),
                            ..Default::default()
                        };
                        surface.present(ui, (r.x, r.y), (r.width as u32, r.height as u32), list)?;
                        surface.show();
                        Ok(Catcher {
                            surface,
                            display: monitor.device.clone(),
                        })
                    })();
                    match result {
                        Ok(c) => self.catchers.push(c),
                        Err(e) => log::warn!("Studio desktop catcher: {e:#}"),
                    }
                }
            }
        }
        for catcher in &self.catchers {
            let _ = catcher.surface.keep_above_desktop();
        }
        if let Some(notice) = &mut self.notice {
            match notice.tick(ui, &self.monitors, bars) {
                Ok(true) => {}
                Ok(false) => self.notice = None,
                Err(e) => {
                    log::warn!("Studio undo chip: {e:#}");
                    self.notice = None;
                }
            }
        }
        if self.gesture.as_ref().is_some_and(|gesture| {
            !targets.iter().any(|t| {
                t.selection == gesture.selection && t.resize.as_ref().is_some_and(|spec|
                    spec.saved_cell == gesture.spec.saved_cell && spec.saved_size == gesture.spec.saved_size
                    && spec.grid == gesture.spec.grid && spec.limits == gesture.spec.limits
                    && spec.origin == gesture.spec.origin && spec.scale == gesture.spec.scale)
            })
        }) {
            self.finish_geometry(false);
        }
        if let Some(gesture) = &mut self.gesture {
            use windows::Win32::Foundation::POINT;
            use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
            use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
            let mut point = POINT::default();
            // SAFETY: read-only pointer state. Polling also covers no-activate background capture.
            let (released, cancelled) = unsafe {
                if GetCursorPos(&mut point).is_ok() {
                    gesture.update(Point::new(point.x as f32, point.y as f32));
                }
                (GetAsyncKeyState(1) >= 0, GetAsyncKeyState(27) < 0)
            };
            if let Some(preview) = &mut self.resize_preview {
                (preview.cell, preview.size) = gesture.pending;
            }
            if let Some(decoration) = self
                .decorations
                .iter_mut()
                .find(|d| d.target.selection == gesture.selection)
            {
                let rect = gesture.spec.rect(gesture.pending.0, gesture.pending.1);
                if let Err(e) =
                    paint_frame(ui, &mut decoration.frame, rect, decoration.target.radius, gesture.spec.scale, true)
                {
                    log::warn!("Studio widget outline: {e:#}");
                }
                let work = self.monitors.iter().find(|m| m.device == gesture.selection.display)
                    .map(|m| chrome::work_area(m, bars));
                let at = chrome::name_position(rect.inflate(3.0 * gesture.spec.scale), decoration.chip_size, work, gesture.spec.scale);
                let _ = decoration.chip.move_to((at.x.round() as i32, at.y.round() as i32));
            }
            if cancelled || released {
                self.finish_geometry(!cancelled);
            } else {
                return;
            }
        }
        if self.hover_until.is_some_and(|at| at <= Instant::now()) {
            self.hovered = None;
            self.hover_until = None;
        }
        let mut wanted: Vec<(Target, bool)> = Vec::new();
        if let Some(selection) = &self.session.selected
            && let Some(target) = targets.iter().find(|t| t.selection == *selection)
        {
            wanted.push((target.clone(), true));
        }
        if let Some(id) = &self.session.highlight
            && let Some(target) = targets
                .iter()
                .find(|t| t.selection.kind == Kind::Widget && t.selection.id == *id)
            && !wanted.iter().any(|(t, _)| t.selection == target.selection)
        {
            wanted.push((target.clone(), false));
        }
        if let Some(hovered) = &self.hovered
            && let Some(target) = targets.iter().find(|t| t.selection == hovered.selection)
        {
            if !wanted.iter().any(|(t, _)| t.selection == target.selection) {
                wanted.push((target.clone(), false));
            }
        }
        if self
            .decorations
            .iter()
            .map(|d| (&d.target, d.selected))
            .ne(wanted.iter().map(|(t, s)| (t, *s)))
        {
            self.decorations.clear();
            for (target, selected) in wanted {
                if let Err(e) = self.decorate(ui, target, selected, bars) {
                    log::warn!("Studio pick chrome: {e:#}");
                }
            }
        }
    }

    /// Runs before controls; only a selected widget's empty face or chip starts a move.
    pub fn event(&mut self, surface: SurfaceId, event: &Event, target: Option<Target>, move_surface: Option<&Surface>) -> bool {
        if !self.active() {
            return false;
        }
        if let Some(notice) = &mut self.notice
            && let Some(undo) = notice.event(surface, event)
        {
            if undo {
                let step = notice.step;
                if let Some((owner, _)) = self.session.owner {
                    wire::send(HWND(owner as _), self.endpoint.hwnd(), &Message::UndoRequest { step });
                }
                self.notice = None;
            }
            return true;
        }
        if let Some(gesture) = &mut self.gesture {
            if gesture.capture == surface {
                match event {
                    Event::Move(p) | Event::Release(p, Button::Left) => {
                        gesture.update(Point::new(p.screen.0 as f32, p.screen.1 as f32));
                        if matches!(event, Event::Release(_, Button::Left)) {
                            self.finish_geometry(true);
                        }
                    }
                    Event::CaptureLost | Event::Cancelled | Event::Key(27) => self.finish_geometry(false),
                    _ => {}
                }
                return true;
            }
        }
        if matches!(event, Event::Press(_, Button::Left)) {
            self.pressed = true;
        }
        if matches!(
            event,
            Event::Release(_, Button::Left) | Event::CaptureLost | Event::Cancelled
        ) {
            self.pressed = false;
        }
        if let Some((decoration, handle, corner)) = self.decorations.iter().find_map(|d| {
            d.handles
                .iter()
                .find(|(s, _)| s.id() == surface)
                .map(|(s, c)| (d, s, *c))
        }) {
            if let Event::Press(p, Button::Left) = event
                && let Some(gesture) = Gesture::new(&decoration.target, handle,
                    Point::new(p.screen.0 as f32, p.screen.1 as f32), Some(corner))
            {
                self.begin_geometry(gesture);
            }
            return true;
        }
        if let Some(decoration) = self.decorations.iter().find(|d| d.chip.id() == surface) {
            let selection = decoration.target.selection.clone();
            match event {
                Event::Move(_) => self.hover_until = None,
                Event::Leave => {
                    self.hover_until = Some(Instant::now() + Duration::from_millis(350))
                }
                Event::Press(p, Button::Left) => {
                    let gesture = Gesture::new(&decoration.target, &decoration.chip,
                        Point::new(p.screen.0 as f32, p.screen.1 as f32), None);
                    self.select(selection);
                    if let Some(gesture) = gesture { self.begin_geometry(gesture); }
                }
                _ => {}
            }
            return true;
        }
        if let Some(catcher) = self.catchers.iter().find(|c| c.surface.id() == surface) {
            if matches!(event, Event::Press(_, Button::Left)) {
                let display = catcher.display.clone();
                self.select(Selection {
                    kind: Kind::Wallpaper,
                    id: "wallpaper".into(),
                    display,
                });
            }
            if matches!(event, Event::Move(_)) && self.hover_until.is_none() {
                self.hover_until = Some(Instant::now() + Duration::from_millis(350));
            }
            return true;
        }
        match event {
            Event::Move(_) => {
                if let Some(target) = target {
                    self.hovered = Some(target);
                    self.hover_until = None;
                }
            }
            Event::Leave => self.hover_until = Some(Instant::now() + Duration::from_millis(350)),
            Event::Press(p, Button::Left) => {
                if let Some(target) = target.filter(|t| t.body_selects) {
                    if self.session.selected.as_ref() == Some(&target.selection)
                        && let Some(surface) = move_surface
                        && let Some(gesture) = Gesture::new(&target, surface,
                            Point::new(p.screen.0 as f32, p.screen.1 as f32), None)
                    {
                        self.begin_geometry(gesture);
                        return true;
                    }
                    self.select(target.selection);
                }
            }
            _ => {}
        }
        false
    }

    fn select(&mut self, selection: Selection) {
        self.gesture = None;
        self.resize_preview = None;
        self.session.selected = Some(selection.clone());
        if let Some((owner, _)) = self.session.owner {
            let taken = wire::send(
                HWND(owner as _),
                self.endpoint.hwnd(),
                &Message::Selected {
                    selection: selection.clone(),
                },
            );
            log::info!("Studio pick {selection:?}, delivered={taken}");
        }
    }

    fn owns(&self, source: HWND) -> bool {
        self.session.owner.is_some_and(|(h, pid)| h == source.0 as isize && wire::process(source) == pid)
    }

    fn reset_transients(&mut self) {
        self.warm_preview = false;
        self.preview = preview::Preview::default();
        self.resize_preview = None;
        self.gesture = None;
        self.hovered = None;
        self.hover_until = None;
        self.decorations.clear();
        self.pressed = false;
        self.notice = None;
    }

    fn begin_geometry(&mut self, gesture: Gesture) {
        self.resize_preview = self.widget_revision.map(|base_revision| resize::Preview {
            selection: gesture.selection.clone(),
            before_cell: gesture.spec.saved_cell, before_size: gesture.spec.saved_size,
            cell: gesture.spec.cell, size: gesture.spec.size, base_revision, saved_revision: None,
        });
        self.gesture = Some(gesture);
    }

    fn finish_geometry(&mut self, commit: bool) {
        let Some(gesture) = self.gesture.take() else {
            return;
        };
        self.pressed = false;
        if let Some(preview) = &mut self.resize_preview {
            (preview.cell, preview.size) = gesture.pending;
        }
        if !commit || gesture.pending == (gesture.spec.cell, gesture.spec.size) { self.resize_preview = None; }
        if commit
            && gesture.pending != (gesture.spec.cell, gesture.spec.size)
            && let Some((owner, _)) = self.session.owner
        {
            let sent = wire::send(
                HWND(owner as _),
                self.endpoint.hwnd(),
                &Message::ResizeWidget {
                    selection: gesture.selection.clone(),
                    before_cell: gesture.spec.saved_cell,
                    before_size: gesture.spec.saved_size,
                    cell: gesture.pending.0,
                    size: gesture.pending.1,
                },
            );
            if !sent { self.resize_preview = None; }
        }
        drop(gesture);
        self.decorations.clear();
    }

    fn decorate(
        &mut self,
        ui: &mut Ui,
        target: Target,
        selected: bool,
        bars: &BarsConfig,
    ) -> Result<()> {
        let Some(parent) = self.anchor else {
            return Ok(());
        };
        let kind = if target.topmost {
            SurfaceKind::Popup { activate: false }
        } else {
            SurfaceKind::DesktopChild { parent }
        };
        let mut frame = Surface::new(kind, "Studio selection frame", Role::Pane)?;
        let mut chip = Surface::new(kind, "Studio selection name", Role::Button)?;
        frame.set_no_activate(true);
        chip.set_no_activate(true);
        frame.set_click_through(true);
        let monitor = self
            .monitors
            .iter()
            .find(|m| m.device == target.selection.display);
        let scale = monitor.map_or(1.0, |m| m.dpi as f32 / 96.0);
        let r = target.rect.inflate(3.0 * scale);
        paint_frame(ui, &mut frame, target.rect, target.radius, scale, selected)?;
        let suffix = if selected {
            ""
        } else if target.selection.kind == Kind::Module {
            " · click to edit"
        } else if target.selection.kind == Kind::Widget {
            " · click to edit; its buttons still work"
        } else {
            " · click empty space to edit"
        };
        let label = format!("{}{suffix}", target.label);
        let style = TextStyle::new("Segoe UI", 11.0 * scale);
        let measured = ui.text.measure(&label, &style);
        let work = monitor.map(|m| chrome::work_area(m, bars));
        let width = (measured.0 + 16.0 * scale)
            .ceil()
            .min(work.map_or(f32::MAX, |r| r.width()));
        let height = (24.0 * scale).ceil();
        let at = chrome::name_position(r, (width, height), work, scale);
        let list = DisplayList {
            ops: vec![
                Op::Fill {
                    rect: Rect::new(0.0, 0.0, width, height),
                    radius: 5.0 * scale,
                    color: Rgba::rgb8([24, 24, 24], 0.98),
                },
                Op::Stroke {
                    rect: Rect::new(scale / 2.0, scale / 2.0, width - scale, height - scale),
                    radius: 5.0 * scale,
                    width: scale,
                    color: Rgba::rgb8([220, 220, 220], 0.35),
                },
                Op::Text {
                    rect: Rect::new(
                        8.0 * scale,
                        (height - measured.1) / 2.0,
                        (width - 16.0 * scale).max(0.0),
                        measured.1,
                    ),
                    text: label.into(),
                    style,
                    color: Rgba::rgb8([245, 245, 245], 1.0),
                },
            ],
            ..Default::default()
        };
        chip.present(
            ui,
            (at.x.round() as i32, at.y.round() as i32),
            (width as u32, height as u32),
            list,
        )?;
        frame.show();
        chip.show();
        if !target.topmost {
            let _ = frame.raise_in_desktop_band();
            let _ = chip.raise_in_desktop_band();
        }
        let mut handles = Vec::new();
        if selected && target.resize.as_ref().is_some_and(|spec| spec.resizable) {
            for corner in resize::CORNERS {
                let rect = resize::handle(r, corner, target.radius + 3.0 * scale, scale);
                let mut surface = Surface::new(kind, "Studio resize corner", Role::Pane)?;
                surface.set_no_activate(true);
                surface.present(
                    ui,
                    (rect.left.round() as i32, rect.top.round() as i32),
                    (rect.width().ceil() as u32, rect.height().ceil() as u32),
                    DisplayList {
                        ops: vec![Op::Fill {
                            rect: Rect::new(0.0, 0.0, rect.width(), rect.height()),
                            radius: 0.0,
                            color: Rgba::HIT,
                        }],
                        ..Default::default()
                    },
                )?;
                surface.show();
                if !target.topmost {
                    let _ = surface.raise_in_desktop_band();
                }
                handles.push((surface, corner));
            }
        }
        self.decorations.push(Decoration {
            frame,
            chip,
            chip_size: (width, height),
            handles,
            target,
            selected,
        });
        Ok(())
    }
}

fn paint_frame(
    ui: &mut Ui,
    surface: &mut Surface,
    target: Rect,
    radius: f32,
    scale: f32,
    selected: bool,
) -> Result<()> {
    let rect = target.inflate(3.0 * scale);
    surface.present(
        ui,
        (
            (rect.left - 3.0 * scale).floor() as i32,
            (rect.top - 3.0 * scale).floor() as i32,
        ),
        (
            (rect.width() + 6.0 * scale).ceil() as u32,
            (rect.height() + 6.0 * scale).ceil() as u32,
        ),
        DisplayList {
            ops: chrome::frame(
                Rect::new(3.0 * scale, 3.0 * scale, rect.width(), rect.height()),
                radius + 3.0 * scale,
                scale,
                selected,
            ),
            ..Default::default()
        },
    )?;
    Ok(())
}

/// Alpha-zero holes preserve desktop icon input; alpha one receives only bare wallpaper.
fn subtract(full: Rect, holes: &[Rect]) -> Vec<Rect> {
    let mut pieces = vec![full];
    for hole in holes {
        pieces = pieces
            .into_iter()
            .flat_map(|r| {
                let h = r.intersect(*hole);
                if h.is_empty() {
                    return vec![r];
                }
                [
                    Rect::new(r.left, r.top, r.width(), h.top - r.top),
                    Rect::new(r.left, h.bottom, r.width(), r.bottom - h.bottom),
                    Rect::new(r.left, h.top, h.left - r.left, h.height()),
                    Rect::new(h.right, h.top, r.right - h.right, h.height()),
                ]
                .into_iter()
                .filter(|r| !r.is_empty())
                .collect()
            })
            .collect();
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;
    fn widget() -> Selection {
        Selection {
            kind: Kind::Widget,
            id: "weather-1".into(),
            display: "display-2".into(),
        }
    }
    #[test]
    fn session_is_owned_and_disconnect_clears_all_transient_state() {
        let mut s = Session::default();
        s.begin(10, 42);
        s.selected = Some(widget());
        s.highlight = Some("clock-1".into());
        s.begin(10, 42);
        assert_eq!(s.selected, Some(widget()));
        s.end(11);
        assert!(s.active());
        s.check(|h, p| h == 10 && p == 42);
        assert!(s.active());
        s.check(|_, _| false);
        assert!(!s.active());
        assert!(s.selected.is_none());
        assert!(s.lifted().is_empty());
    }
    #[test]
    fn replacement_and_hwnd_reuse_start_a_fresh_session() {
        let mut s = Session::default();
        s.begin(10, 42);
        s.selected = Some(widget());
        s.begin(10, 43);
        assert!(s.selected.is_none());
        s.highlight = Some("clock-1".into());
        s.begin(11, 43);
        assert!(s.highlight.is_none());
        s.end(11);
        assert!(!s.active());
    }
    #[test]
    fn wallpaper_hit_regions_exclude_icons_and_use_exclusive_edges() {
        let full = Rect::new(-100.0, 0.0, 200.0, 100.0);
        let icon = Rect::new(-30.0, 20.0, 40.0, 40.0);
        let pieces = subtract(full, &[icon, Rect::new(200.0, 0.0, 20.0, 20.0)]);
        for x in -100..100 {
            for y in 0..100 {
                let p = Point::new(x as f32, y as f32);
                assert_eq!(
                    pieces.iter().filter(|r| r.contains(p)).count(),
                    usize::from(!icon.contains(p))
                );
            }
        }
        assert!(!pieces.iter().any(|r| r.contains(Point::new(100.0, 0.0))));
    }
    #[test]
    fn selected_and_hovered_rows_lift_only_widgets() {
        let mut s = Session::default();
        s.begin(10, 42);
        s.selected = Some(widget());
        s.highlight = Some("clock".into());
        assert_eq!(
            s.lifted(),
            BTreeSet::from(["weather-1".into(), "clock".into()])
        );
        s.selected.as_mut().unwrap().kind = Kind::Bar;
        assert_eq!(s.lifted(), BTreeSet::from(["clock".into()]));
    }
}
