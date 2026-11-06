//! mypaper Studio: the settings app, native on `mypaper-ui` (software Direct2D, one window, no
//! WebView, no GPU device). It edits `config.json`, `bars.json`, `widgets.json` and your own
//! wallpaper folders; mypaper's programs watch those files and apply changes live. Its previews
//! draw with the shell's own views, and the engine draws their glass (plans/native/
//! studio-redesign.md).
//!
//! One Studio at a time: a second launch brings the first to the front. Closing it ends the
//! process (nothing of Studio stays running).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
mod dialogs;
mod docs;
mod glass;
mod images;
mod library;
mod look;
mod editing;
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

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

use anyhow::{Context, Result};
use mypaper_ui::{Kind, Role, Surface, Ui, next_event};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, LPARAM, WAIT_OBJECT_0, WPARAM};
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, FindWindowW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
    PeekMessageW, PostThreadMessageW, QS_ALLINPUT, SW_RESTORE, SetForegroundWindow, ShowWindow,
    TranslateMessage, WM_QUIT,
};
use windows::core::w;

use crate::app::{App, Msg, WM_STUDIO};
use mypaper_bars::wake as mypaper_bars_wake;

// This branch runs alongside the installed Studio until the owner chooses to replace it.
const TITLE: &str = "mypaper Studio (new)";

/// Wakes the UI thread's loop (workers post after sending a [`Msg`]).
pub fn wake(thread: u32) {
    // SAFETY: posting to a thread id is always safe; failure means it's gone.
    let _ = unsafe { PostThreadMessageW(thread, WM_STUDIO, WPARAM(0), LPARAM(0)) };
}

fn main() -> ExitCode {
    mypaper_desktop::init_logging();
    mypaper_desktop::enable_dpi_awareness();
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
    menu: Option<mypaper_ui::Menu<usize>>,
    picking: Option<actions::Choices>,
    /// The DWM frame last set: dark, border colour.
    frame: Option<(bool, mypaper_ui::Rgba)>,
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
            .is_some_and(|t| matches!(t.behavior, mypaper_ui::controls::Behavior::Field));
        let resize = matches!(app.drag, Some(crate::app::Drag::Inspector { .. }))
            || app.input.hover == Some(mypaper_ui::controls::key("inspector-resize"));
        surface.set_cursor(if resize { mypaper_ui::Cursor::ResizeHorizontal } else if over_field { mypaper_ui::Cursor::Text } else { mypaper_ui::Cursor::Arrow });
    }

    /// The window's border and caption follow the palette (light/dark, contrast) live. DWM
    /// wants an opaque border: the edge wash over the window's chrome.
    fn sync_frame(&mut self) {
        let p = panel::palette_for(&self.app);
        let frame = (p.dark, mypaper_ui::theme::over(p.edge, p.chrome));
        if self.frame != Some(frame) {
            self.surface.set_frame(frame.0, frame.1);
            self.frame = Some(frame);
        }
    }

    /// Opens the kit's menu under the select that asked for it.
    fn open_picker(&mut self) {
        let Some((rect, choices)) = self.app.picking.take() else {
            return;
        };
        if self.menu.is_none() {
            self.menu = mypaper_ui::Menu::new("Options")
                .inspect_err(|e| log::error!("select menu: {e:#}"))
                .ok();
        }
        let Some(menu) = self.menu.as_mut() else { return };
        let items = choices
            .options
            .iter()
            .enumerate()
            .map(|(i, (value, label))| mypaper_ui::menu::Entry::check(label, i, choices.current.as_ref() == Some(value)))
            .collect();
        let mut at = windows::Win32::Foundation::POINT {
            x: rect.left as i32,
            y: (rect.bottom + self.app.px(4.0)) as i32,
        };
        // SAFETY: our own window; `at` is a valid out pointer.
        let _ = unsafe { windows::Win32::Graphics::Gdi::ClientToScreen(self.surface.hwnd(), &mut at) };
        let p = panel::palette_for(&self.app);
        let mut tokens = p.theme().tokens();
        // A popover over the window: the card fill, Studio's selected wash, no hairline in dark.
        tokens.material.tint = p.panel;
        tokens.colors.hover = p.selected;
        let screen = mypaper_ui::menu::screen_at(at.x, at.y);
        match menu.open(&mut self.ui, items, (at.x, at.y), screen, &tokens) {
            Ok(()) => self.picking = Some(choices),
            Err(e) => log::error!("select menu: {e:#}"),
        }
    }

    /// The select menu closed: apply the choice and give the window its focus back (unless
    /// the user clicked into another app).
    fn picked(&mut self, outcome: mypaper_ui::menu::Outcome<usize>) {
        let choices = self.picking.take();
        if let (mypaper_ui::menu::Outcome::Chosen(i), Some(choices)) = (&outcome, &choices) {
            self.app.chose(choices, *i);
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
            if let mypaper_ui::Event::DpiChanged(dpi) = event {
                self.app.scale = dpi as f32 / 96.0;
                self.app.dirty = true;
                continue;
            }
            let accessibility = if let mypaper_ui::Event::Invoke(hit) = event {
                self.surface.take_accessibility_action(hit).map(|action| (hit, action))
            } else { None };
            if let Some((hit, action)) = accessibility { panel::accessibility(&mut self.app, hit, action); }
            else { self.app.event(event); }
            // An event may have opened or closed the live field: attach it to the window.
            self.sync_field();
            self.open_picker();
        }
    }

    fn sync_field(&mut self) {
        let field = self.app.field.as_ref().map(|f| f.field.clone());
        self.surface.set_text_field(field);
    }
}

fn run() -> Result<()> {
    let panel_mode = panel::enabled();
    let title = if panel_mode { panel::TITLE } else { TITLE };
    // SAFETY: a named mutex held (never closed) for the life of the process.
    unsafe {
        let name = if panel_mode { w!("Local\\mypaper-studio-panel-new") } else { w!("Local\\mypaper-studio-window-new") };
        let _mutex = CreateMutexW(None, true, name).context("mutex")?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(hwnd) = FindWindowW(w!("mypaper.ui.app"), &windows::core::HSTRING::from(title)) {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
            log::info!("Studio is already open; brought it to the front");
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
    app.images.borrow_mut().set_budget(2 << 20);
    app.window = Some(surface.hwnd());
    app.inbox = Some(tx.clone());
    app.thread = thread;
    app.images.borrow_mut().connect(tx.clone(), thread);
    surface.set_icon_png(include_bytes!("../assets/icon-32.png"));
    let (bounds, maximized) = if panel_mode { (panel::placement(&app), false) } else { placement(&app, scale) };
    surface.place(bounds);
    panel::round_window(surface.hwnd());
    if panel_mode {
        panel::fit_client(&surface, bounds);
        panel::round_window(surface.hwnd());
        app.scale = surface.scale();
    }
    let studio = Rc::new(RefCell::new(Studio { app, ui, surface, menu: None, picking: None, frame: None }));
    {
        let mut s = studio.borrow_mut();
        s.sync_frame();
        s.app.size = s.surface.client_size();
        s.render();
        s.surface.show_active(maximized);
    }
    // `READY component=studio open_ms=…`, counted from process creation (the ≤300 ms gate).
    mypaper_desktop::startup::ready("studio", "open_ms");
    let _watch = watch::start(tx, thread).inspect_err(|e| log::warn!("not following outside edits: {e:#}"));
    // While Windows' size/move loop runs, the app's stops: render from inside it.
    let hook = Rc::downgrade(&studio);
    mypaper_ui::set_modal_hook(Some(Box::new(move || {
        if let Some(s) = hook.upgrade()
            && let Ok(mut s) = s.try_borrow_mut()
        {
            s.drain();
            s.app.dirty = true;
            s.render();
        }
    })));
    let result = pump(&studio, &rx);
    mypaper_ui::set_modal_hook(None);
    let mut s = studio.borrow_mut();
    if !panel_mode {
        remember_placement(&mut s);
    }
    peek::end(&mut s.app);
    result
}

/// The message loop: sleeps until input, a worker's message, the engine's glass answer, or the
/// next timer (a toast, a live status refresh while visible).
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
                        mypaper_bars_wake::WM_WAKE => {
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
    }
}

/// Where the window opens: where it was left, else 1280×800 centred on the main display.
fn placement(app: &App, scale: f32) -> ((i32, i32, i32, i32), bool) {
    let saved = app.get(docs::DocId::Studio, "/window").cloned();
    if let Some(w) = saved
        && let (Some(x), Some(y), Some(width), Some(height)) = (
            w.get("x").and_then(serde_json::Value::as_i64),
            w.get("y").and_then(serde_json::Value::as_i64),
            w.get("width").and_then(serde_json::Value::as_i64),
            w.get("height").and_then(serde_json::Value::as_i64),
        )
        && app.monitors.iter().any(|m| {
            let (cx, cy) = (x as i32 + width as i32 / 2, y as i32 + height as i32 / 2);
            cx >= m.work.x && cx < m.work.x + m.work.width && cy >= m.work.y && cy < m.work.y + m.work.height
        })
    {
        let max = w.get("maximized").and_then(serde_json::Value::as_bool).unwrap_or(false);
        return ((x as i32, y as i32, width as i32, height as i32), max);
    }
    let work = app
        .monitors
        .iter()
        .find(|m| m.primary)
        .map_or(mypaper_desktop::Rect { x: 0, y: 0, width: 1920, height: 1080 }, |m| m.work);
    let (w, h) = (((1280.0 * scale) as i32).min(work.width), ((800.0 * scale) as i32).min(work.height));
    ((work.x + (work.width - w) / 2, work.y + (work.height - h) / 2, w, h), false)
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
    let r = wp.rcNormalPosition;
    let value = serde_json::json!({
        "x": r.left, "y": r.top, "width": r.right - r.left, "height": r.bottom - r.top,
        "maximized": wp.showCmd == SW_SHOWMAXIMIZED.0 as u32,
    });
    s.app.apply("Window", vec![docs::Op {
        doc: docs::DocId::Studio,
        path: "/window".into(),
        value: Some(value),
    }]);
}
