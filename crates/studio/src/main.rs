//! Glaze Studio: the settings app, native on `glaze-ui` (software Direct2D, one window, no
//! WebView, no GPU device). It edits `config.json`, `bars.json`, `widgets.json` and your own
//! wallpaper folders; Glaze's programs watch those files and apply changes live. Its previews
//! draw with the shell's own views, and the engine draws their glass (plans/native/
//! studio-redesign.md).
//!
//! One Studio at a time: a second launch brings the first to the front. Closing it ends the
//! process (nothing of Studio stays running).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
mod builder;
mod dialogs;
mod docs;
mod glass;
mod images;
mod library;
mod look;
mod editing;
mod edit_mode;
mod onboarding;
mod panel;
mod peek;
mod search;
mod settings;
mod stage;
mod system;
mod theme;
mod ui;
mod view;
mod watch;

#[cfg(test)]
mod goldens;
#[cfg(test)]
mod pump_tests;

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

use anyhow::{Context, Result};
use glaze_ui::{Kind, Role, Surface, Ui, next_event};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, LPARAM, WAIT_OBJECT_0, WPARAM};
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, FindWindowW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
    PeekMessageW, PostThreadMessageW, QS_ALLINPUT, SW_RESTORE, SetForegroundWindow, ShowWindow,
    TranslateMessage, WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
};
use windows::core::w;

use crate::app::{App, Msg, WM_STUDIO};
use glaze_bars::wake as glaze_bars_wake;

const TITLE: &str = "Glaze Studio";
/// Decoded pictures the window keeps (RAM counts as much as CPU).
const IMAGES: usize = 2 << 20;
/// Full-screen onboarding shows a few large pictures every animation frame: under the window's
/// budget they evict each other and flicker. Raised only while it is on screen.
const ONBOARDING_IMAGES: usize = 16 << 20;

/// Wakes the UI thread's loop (workers post after sending a [`Msg`]).
pub fn wake(thread: u32) {
    // SAFETY: posting to a thread id is always safe; failure means it's gone.
    let _ = unsafe { PostThreadMessageW(thread, WM_STUDIO, WPARAM(0), LPARAM(0)) };
}

fn main() -> ExitCode {
    glaze_desktop::init_logging();
    glaze_desktop::enable_dpi_awareness();
    // Glaze's window names: a second launch finds this window by its `glaze.ui.app` class.
    let _ = glaze_ui::set_identity(glaze_ui::Identity {
        prefix: "glaze.ui",
        desktop_classes: &["glaze.wallpaper.glass"],
    });
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Everything the loop owns; shared with the live-resize hook.
struct Studio {
    app: App,
    ui: Ui,
    surface: Surface,
    /// The selects' menu (the kit's), made on first use so it costs nothing at startup, and
    /// the options it is showing.
    menu: Option<glaze_ui::Menu<usize>>,
    picking: Option<Picking>,
    /// Edit mode on the desktop, while it lasts.
    edit: Option<edit_mode::Session>,
    /// The DWM frame last set: dark, border colour.
    frame: Option<(bool, glaze_ui::Rgba)>,
    /// The full-screen first run, while it covers the displays.
    onboarding: Option<onboarding::Session>,
}

/// What the open menu answers to: a select's choices, or an action menu's rows by index.
enum Picking {
    Choices(actions::Choices),
    Actions(Vec<actions::Action>),
}

/// The kit's rows for an action menu; each chosen row's index finds its action in `actions`.
fn menu_entries(items: &[actions::MenuItem], actions: &mut Vec<actions::Action>) -> Vec<glaze_ui::menu::Entry<usize>> {
    use glaze_ui::menu::{Entry, MenuIcon};
    items
        .iter()
        .map(|item| match item {
            actions::MenuItem::Separator => Entry::Separator,
            actions::MenuItem::Submenu { label, items } => Entry::Submenu { label: label.clone(), items: menu_entries(items, actions) },
            actions::MenuItem::Item { label, icon, checked, enabled, action } => {
                actions.push(action.clone());
                let entry = Entry::Action { label: label.clone(), command: actions.len() - 1, checked: *checked, enabled: *enabled };
                match (icon, checked) {
                    (Some(glyph), None) => entry.with_icon(MenuIcon::Glyph(*glyph)),
                    _ => entry,
                }
            }
        })
        .collect()
}

impl Studio {
    /// Builds and shows a frame when something changed.
    fn render(&mut self) {
        if !self.app.dirty {
            return;
        }
        self.sync_frame();
        let Studio { app, ui, surface, .. } = self;
        let size = surface.client_size();
        if size.0 == 0 || size.1 == 0 {
            return;
        }
        app.size = size;
        panel::prepare_frame(app, surface);
        let docked = !app.panel.as_ref().unwrap().windowed;
        let mut built = if docked {
            panel::build(app, &mut ui.text)
        } else {
            view::build(app, ui)
        };
        if docked && panel::fit_height(app, surface, &built) {
            built = panel::build(app, &mut ui.text);
        }
        let size = surface.client_size();
        surface.set_chrome(&built.chrome);
        surface.set_background(panel::palette_for(app).chrome);
        let anchors = std::mem::take(&mut *app.anchors.borrow_mut());
        app.frame = app::Frame {
            targets: built.targets,
            scrolls: built.scrolls,
            regions: built.regions,
            anchors,
            overlay_from: built.overlay_from,
        };
        surface.set_accessibility_semantics(built.semantics);
        if let Err(e) = surface.present(ui, (0, 0), size, built.list) {
            log::error!("drawing: {e:#}");
        }
        app.dirty = false;
        if app.panel.is_some() { panel::after_present(app); }
        app.settle_reveal();
        // Text cursor over fields, an arrow elsewhere.
        let over_field = app
            .input
            .hover
            .and_then(|k| app.frame.targets.iter().find(|t| t.key == k))
            .is_some_and(|t| matches!(t.behavior, glaze_ui::controls::Behavior::Field));
        let resize = matches!(app.drag, Some(crate::app::Drag::Inspector { .. }))
            || app.input.hover == Some(glaze_ui::controls::key("inspector-resize"));
        surface.set_cursor(if resize { glaze_ui::Cursor::ResizeHorizontal } else if over_field { glaze_ui::Cursor::Text } else { glaze_ui::Cursor::Arrow });
    }

    /// The window's border and caption follow the palette (light/dark, contrast) live. DWM
    /// wants an opaque border: the edge wash over the window's chrome.
    fn sync_frame(&mut self) {
        // Full-screen onboarding has no frame; it is set again when Studio becomes a window.
        if self.onboarding.is_some() {
            return;
        }
        let p = panel::palette_for(&self.app);
        let frame = (p.dark, glaze_ui::theme::over(p.edge, p.chrome));
        if self.frame != Some(frame) {
            self.surface.set_frame(frame.0, frame.1);
            self.frame = Some(frame);
        }
    }

    /// Opens the kit's menu under the select that asked for it.
    fn open_picker(&mut self) {
        let (rect, picking, items) = if let Some((rect, choices)) = self.app.picking.take() {
            let items = choices
                .options
                .iter()
                .enumerate()
                .map(|(i, (value, label))| glaze_ui::menu::Entry::check(label, i, choices.current.as_ref() == Some(value)))
                .collect();
            (rect, Picking::Choices(choices), items)
        } else if let Some((rect, rows)) = self.app.menu.take() {
            let mut actions = Vec::new();
            let items = menu_entries(&rows, &mut actions);
            (rect, Picking::Actions(actions), items)
        } else {
            return;
        };
        if self.menu.is_none() {
            self.menu = glaze_ui::Menu::new("Options")
                .inspect_err(|e| log::error!("select menu: {e:#}"))
                .ok();
        }
        let Some(menu) = self.menu.as_mut() else { return };
        let p = panel::palette_for(&self.app);
        let mut tokens = p.theme().tokens();
        // A popover over the window: the card fill, Studio's selected wash, no hairline in dark.
        tokens.material.tint = p.panel;
        tokens.colors.hover = p.selected;
        // Menus line up with their control's right edge: dropdown values sit at a row's end and
        // "…" buttons at a panel's, so the menu opens over the page instead of past it.
        let mut at = windows::Win32::Foundation::POINT {
            x: rect.left as i32,
            y: (rect.bottom + self.app.px(4.0)) as i32,
        };
        // SAFETY: our own window; `at` is a valid out pointer.
        let _ = unsafe { windows::Win32::Graphics::Gdi::ClientToScreen(self.surface.hwnd(), &mut at) };
        let screen = glaze_ui::menu::screen_at(at.x, at.y);
        let scale = screen.dpi.max(96) as f32 / 96.0 * tokens.scale.clamp(0.5, 2.0);
        let width = glaze_ui::menu::layout(&items, &tokens, scale, &mut self.ui.text).size().0;
        at.x += rect.width() as i32 - width;
        match menu.open(&mut self.ui, items, (at.x, at.y), screen, &tokens) {
            Ok(()) => self.picking = Some(picking),
            Err(e) => log::error!("select menu: {e:#}"),
        }
    }

    /// The select menu closed: apply the choice and give the window its focus back (unless
    /// the user clicked into another app).
    fn picked(&mut self, outcome: glaze_ui::menu::Outcome<usize>) {
        match (&outcome, self.picking.take()) {
            (glaze_ui::menu::Outcome::Chosen(i), Some(Picking::Choices(choices))) => self.app.chose(&choices, *i),
            (glaze_ui::menu::Outcome::Chosen(i), Some(Picking::Actions(actions))) => {
                if let Some(action) = actions.get(*i) {
                    self.app.run(action.clone());
                }
            }
            _ => {}
        }
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
        // SAFETY: plain window queries; activating our own window while it is ours to give.
        unsafe {
            let front = GetForegroundWindow();
            let mut pid = 0;
            GetWindowThreadProcessId(front, Some(&mut pid));
            if front.is_invalid() || pid == std::process::id() {
                let _ = SetForegroundWindow(self.surface.hwnd());
            }
        }
        self.app.dirty = true;
    }

    /// Handles the window's queued input.
    fn drain(&mut self) {
        let id = self.surface.id();
        while let Some((of, event)) = next_event() {
            if let Some(menu) = self.menu.as_mut()
                && menu.owns(of)
            {
                if let Some(outcome) = menu.handle(&mut self.ui, of, &event) {
                    self.picked(outcome);
                }
                continue;
            }
            if of != id {
                continue;
            }
            if let glaze_ui::Event::DpiChanged(dpi) = event {
                self.app.scale = dpi as f32 / 96.0;
                self.app.dirty = true;
                continue;
            }
            let accessibility = if let glaze_ui::Event::Invoke(hit) = event {
                self.surface.take_accessibility_action(hit).map(|action| (hit, action))
            } else { None };
            if let Some((hit, action)) = accessibility { panel::accessibility(&mut self.app, hit, action); }
            else { self.app.event(event); }
            // An event may have opened or closed the live field: attach it to the window.
            self.sync_field();
            self.open_picker();
            self.apply_edit_request();
        }
    }

    /// Enters or leaves edit mode when a page asked for it: the window becomes the docked
    /// panel over the cleared desktop, and goes back to where it was afterwards.
    fn apply_edit_request(&mut self) {
        let Some(on) = self.app.panel.as_mut().and_then(|state| state.edit_request.take()) else { return };
        if self.app.fixture || on == self.app.panel.as_ref().unwrap().editing {
            return;
        }
        if on {
            let state = self.app.panel.as_mut().unwrap();
            // Settings pages have nothing on the desktop: start at its overview.
            if !crate::view::has_stage(state.page) && state.page != panel::Page::Wallpapers {
                state.page = panel::Page::Root;
            }
            state.windowed = false;
            state.editing = true;
            let bounds = panel::placement(&self.app);
            self.edit = Some(edit_mode::Session::enter(self.surface.hwnd(), bounds));
            panel::fit_client(&self.surface, bounds);
        } else {
            let state = self.app.panel.as_mut().unwrap();
            state.windowed = true;
            state.editing = false;
            if let Some(session) = self.edit.take() {
                session.exit();
            }
        }
        self.app.scale = self.surface.scale();
        self.app.input.focus = None;
        self.frame = None;
        self.app.dirty = true;
        self.render();
    }

    /// Onboarding finished or was skipped: fade the main display out, show Studio as a normal
    /// window where it opens by default, and fade every display back in.
    fn end_onboarding(&mut self) {
        let Some(session) = self.onboarding.take() else { return };
        let scale = self.surface.scale();
        let (bounds, _) = placement(&self.app, scale);
        session.cover(self.surface.hwnd(), bounds);
        if let Some(panel) = self.app.panel.as_mut() {
            panel.intro = None;
        }
        self.app.images.borrow_mut().set_budget(IMAGES);
        self.frame = None;
        self.app.scale = self.surface.scale();
        self.app.dirty = true;
        self.render();
        session.reveal();
    }

    fn sync_field(&mut self) {
        let field = self.app.field.as_ref().map(|f| f.field.clone());
        self.surface.set_text_field(field);
    }
}

fn run() -> Result<()> {
    let panel_mode = panel::enabled();
    // UI Automation can inspect and invoke Studio without interrupting the user's foreground app.
    let no_activate = std::env::args().any(|arg| arg == "--no-activate");
    let title = if panel_mode { panel::TITLE } else { TITLE };
    // SAFETY: a named mutex held (never closed) for the life of the process.
    unsafe {
        let name = if panel_mode { w!("Local\\glaze-studio-panel") } else { w!("Local\\glaze-studio") };
        let _mutex = CreateMutexW(None, true, name).context("mutex")?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if !no_activate
                && let Ok(hwnd) = FindWindowW(w!("glaze.ui.app"), &windows::core::HSTRING::from(title))
            {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
            log::info!("Studio is already open{}", if no_activate { "" } else { "; brought it to the front" });
            return Ok(());
        }
    }
    let mut ui = Ui::new()?;
    // Studio shows a few hundred labels at once: keep them all laid out.
    ui.text.set_capacity(64);
    // UI Automation loads on its first request rather than before the first paint.
    let min = if panel_mode { (360, 160) } else { (960, 640) };
    let surface = Surface::new_deferred_accessibility(Kind::App { min }, title, Role::Window)?;
    let scale = surface.scale();
    // SAFETY: the calling thread's id.
    let thread = unsafe { GetCurrentThreadId() };
    let (tx, rx) = channel::<Msg>();
    let mut app = App::new(&ui.text, scale, (1280, 800));
    app.images.borrow_mut().set_budget(IMAGES);
    app.window = Some(surface.hwnd());
    app.inbox = Some(tx.clone());
    app.thread = thread;
    app.images.borrow_mut().connect(tx.clone(), thread);
    set_window_icons(surface.hwnd(), scale)?;
    let (bounds, maximized) = if panel_mode { (panel::placement(&app), false) } else { placement(&app, scale) };
    // (First run moves the window over the whole main display once the curtains are down.)
    surface.place(bounds);
    panel::round_window(surface.hwnd());
    if panel_mode {
        panel::fit_client(&surface, bounds);
        panel::round_window(surface.hwnd());
        app.scale = surface.scale();
    }
    // `--onboarding` shows first run again on a set-up PC (every pick still applies, and undoes).
    if !panel_mode && std::env::args().any(|arg| arg == "--onboarding") {
        app.panel.as_mut().unwrap().page = panel::Page::FirstRun;
        app.overlay = Some(app::Overlay::FirstRun { step: 0, starter: None, taskbar: 0 });
    }
    let onboarding = (!panel_mode && !no_activate && matches!(app.overlay, Some(app::Overlay::FirstRun { .. })))
        .then(|| onboarding::Session::begin(surface.hwnd(), &app.monitors));
    if onboarding.is_some() {
        app.panel.as_mut().unwrap().intro = Some(Instant::now());
        app.images.borrow_mut().set_budget(ONBOARDING_IMAGES);
        app.scale = surface.scale();
    }
    let studio = Rc::new(RefCell::new(Studio { app, ui, surface, menu: None, picking: None, edit: None, frame: None, onboarding }));
    {
        let mut s = studio.borrow_mut();
        s.sync_frame();
        s.app.size = s.surface.client_size();
        s.render();
        if no_activate {
            s.surface.show();
        } else {
            // Onboarding is already placed over the whole display: never maximize it into the work area.
            s.surface.show_active(maximized && s.onboarding.is_none());
        }
    }
    // `READY component=studio open_ms=…`, counted from process creation (the ≤300 ms gate).
    glaze_desktop::startup::ready("studio", "open_ms");
    let _watch = watch::start(tx, thread).inspect_err(|e| log::warn!("not following outside edits: {e:#}"));
    // While Windows' size/move loop runs, the app's stops: render from inside it.
    let hook = Rc::downgrade(&studio);
    glaze_ui::set_modal_hook(Some(Box::new(move || {
        if let Some(s) = hook.upgrade()
            && let Ok(mut s) = s.try_borrow_mut()
        {
            s.drain();
            s.app.dirty = true;
            s.render();
        }
    })));
    let result = pump(&studio, &rx);
    glaze_ui::set_modal_hook(None);
    let mut s = studio.borrow_mut();
    // Closing mid-edit: windows come back and Studio's own place is the window's, not the panel's.
    if let Some(session) = s.edit.take() {
        session.exit();
    }
    // A window closed mid-onboarding was full screen: that is no place to reopen at.
    if !panel_mode && s.onboarding.is_none() {
        remember_placement(&mut s);
    }
    peek::end(&mut s.app);
    result
}

/// The class and task switchers use the same embedded artwork as executable icon lookup.
fn set_window_icons(hwnd: windows::Win32::Foundation::HWND, scale: f32) -> Result<()> {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        GCLP_HICON, GCLP_HICONSM, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED, LoadImageW,
        SendMessageW, SetClassLongPtrW, WM_SETICON,
    };
    // SAFETY: resource 101 is linked into our exe. LR_SHARED keeps both icons alive for
    // the class and window lifetime; Windows owns them and they must not be destroyed.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        for (kind, class, size) in [
            (ICON_BIG, GCLP_HICON, 32.0),
            (ICON_SMALL, GCLP_HICONSM, 16.0),
        ] {
            let size = (size * scale).round() as i32;
            let icon = LoadImageW(
                Some(instance.into()),
                windows::core::PCWSTR(101usize as *const u16),
                IMAGE_ICON,
                size,
                size,
                LR_SHARED,
            )
            .context("Studio icon resource")?;
            SetClassLongPtrW(hwnd, class, icon.0 as isize);
            SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(kind as usize)),
                Some(LPARAM(icon.0 as isize)),
            );
        }
    }
    Ok(())
}

/// The message loop sleeps until input, a worker's message or the next timer.
fn pump(studio: &Rc<RefCell<Studio>>, rx: &Receiver<Msg>) -> Result<()> {
    let mut msg = MSG::default();
    loop {
        let (handles, timeout): (Vec<HANDLE>, u32) = {
            let s = studio.borrow();
            let handles = s.app.glass.borrow().handle().into_iter().collect();
            let timeout = s.app.deadline().map_or(u32::MAX, |d| {
                d.saturating_duration_since(Instant::now()).as_millis().min(u32::MAX as u128 - 1) as u32
            });
            (handles, timeout)
        };
        // SAFETY: the handles stay open while waited on (owned by `glass`).
        let woke = unsafe {
            MsgWaitForMultipleObjectsEx(
                (!handles.is_empty()).then_some(handles.as_slice()),
                timeout,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            )
        };
        if !handles.is_empty() && woke == WAIT_OBJECT_0 {
            let s = studio.borrow();
            if s.app.glass.borrow_mut().arrived() {
                drop(s);
                studio.borrow_mut().app.dirty = true;
            }
        }
        // SAFETY: the standard pump on the thread that owns the window.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    return Ok(());
                }
                if msg.hwnd.is_invalid() {
                    match msg.message {
                        WM_STUDIO => continue,
                        // The shell's icon service (in the stage) has news.
                        glaze_bars_wake::WM_WAKE => {
                            let s = studio.borrow();
                            let rebuilt = s.app.stage.borrow_mut().icons_arrived();
                            drop(s);
                            if rebuilt {
                                studio.borrow_mut().app.dirty = true;
                            }
                            continue;
                        }
                        peek::WM_PEEK_END => {
                            studio.borrow_mut().app.message(Msg::PeekEnded);
                            continue;
                        }
                        _ => {}
                    }
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
                // Shortcuts read this message's GetKeyState. Consume them before removing
                // a queued key-up, which would otherwise erase Ctrl/Shift/Alt from the chord.
                if matches!(msg.message, WM_KEYDOWN | WM_SYSKEYDOWN) {
                    studio.borrow_mut().drain();
                }
            }
        }
        let mut s = studio.borrow_mut();
        while let Ok(m) = rx.try_recv() {
            s.app.message(m);
        }
        s.drain();
        s.app.tick();
        if s.app.quit {
            return Ok(());
        }
        s.render();
        if s.onboarding.is_some() && !matches!(s.app.overlay, Some(app::Overlay::FirstRun { .. })) {
            s.end_onboarding();
        }
    }
}

/// Where the window opens: where it was left (kept inside that display's work area, so the top
/// edge can always be grabbed), else 1280×800 centred on the main display.
fn placement(app: &App, scale: f32) -> ((i32, i32, i32, i32), bool) {
    let saved = app.get(docs::DocId::Studio, "/window").cloned();
    if let Some(w) = saved
        && let (Some(x), Some(y), Some(width), Some(height)) = (
            w.get("x").and_then(serde_json::Value::as_i64),
            w.get("y").and_then(serde_json::Value::as_i64),
            w.get("width").and_then(serde_json::Value::as_i64),
            w.get("height").and_then(serde_json::Value::as_i64),
        )
        && let Some(m) = app.monitors.iter().find(|m| {
            let (cx, cy) = (x as i32 + width as i32 / 2, y as i32 + height as i32 / 2);
            cx >= m.work.x && cx < m.work.x + m.work.width && cy >= m.work.y && cy < m.work.y + m.work.height
        })
    {
        let max = w.get("maximized").and_then(serde_json::Value::as_bool).unwrap_or(false);
        return (inside(m.work, (x as i32, y as i32, width as i32, height as i32)), max);
    }
    let work = app
        .monitors
        .iter()
        .find(|m| m.primary)
        .map_or(glaze_desktop::Rect { x: 0, y: 0, width: 1920, height: 1080 }, |m| m.work);
    let (w, h) = (((1280.0 * scale) as i32).min(work.width), ((800.0 * scale) as i32).min(work.height));
    ((work.x + (work.width - w) / 2, work.y + (work.height - h) / 2, w, h), false)
}

/// `rect` shrunk to fit `work` and moved fully inside it.
fn inside(work: glaze_desktop::Rect, (x, y, w, h): (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    let (w, h) = (w.min(work.width), h.min(work.height));
    (
        x.clamp(work.x, work.x + work.width - w),
        y.clamp(work.y, work.y + work.height - h),
        w,
        h,
    )
}

/// What turns `hwnd`'s workspace coordinates into screen coordinates: its monitor's work-area
/// origin minus the monitor origin.
fn workspace_offset(hwnd: windows::Win32::Foundation::HWND) -> (i32, i32) {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    // SAFETY: queries on our own window into a sized local.
    let ok = unsafe { GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info) }.as_bool();
    if !ok {
        return (0, 0);
    }
    (info.rcWork.left - info.rcMonitor.left, info.rcWork.top - info.rcMonitor.top)
}

fn remember_placement(s: &mut Studio) {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, SW_SHOWMAXIMIZED, WINDOWPLACEMENT};
    let mut wp = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    // SAFETY: a query on our own window into a sized local.
    if unsafe { GetWindowPlacement(s.surface.hwnd(), &mut wp) }.is_err() {
        return;
    }
    // `rcNormalPosition` is in workspace coordinates: relative to the work area of the window's
    // monitor, not the screen. Saved as screen coordinates, the window crept up by the top bar's
    // height on every close until its title bar sat behind the bar.
    let (dx, dy) = workspace_offset(s.surface.hwnd());
    let r = wp.rcNormalPosition;
    let value = serde_json::json!({
        "x": r.left + dx, "y": r.top + dy, "width": r.right - r.left, "height": r.bottom - r.top,
        "maximized": wp.showCmd == SW_SHOWMAXIMIZED.0 as u32,
    });
    s.app.apply("Window", vec![docs::Op {
        doc: docs::DocId::Studio,
        path: "/window".into(),
        value: Some(value),
    }]);
}

#[cfg(test)]
mod placement_tests {
    use super::inside;
    use glaze_desktop::Rect;

    #[test]
    fn a_window_left_above_the_top_bar_opens_below_it() {
        // A display left of the main one with a 37 px top bar: the old saves crept to y = -39.
        let work = Rect { x: -2560, y: 37, width: 2560, height: 1403 };
        assert_eq!(inside(work, (-2062, -39, 1880, 980)), (-2062, 37, 1880, 980));
        // Too big for the work area: shrunk to fit, not pushed off the far edge.
        assert_eq!(inside(work, (-2560, 37, 3000, 2000)), (-2560, 37, 2560, 1403));
    }
}
