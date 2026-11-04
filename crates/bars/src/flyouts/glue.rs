//! The flyouts' and the tray host's part of the main loop (docs/bars.md, "Flyouts"): routing a
//! flyout's requests, delivering what the providers finished, the Windows key, the keyboard for
//! the launcher, and which tray source runs. A child module of `app`, so it works on `App`
//! directly; app.rs only calls in.

use super::*;
use crate::bridge::{Panel, bar_modules};
use crate::flyouts::winkey::Target;
use mypaper_bars::{Module, ModuleRef};

/// A Windows key press the main loop gets to later than this (it was busy) is dropped: a
/// launcher popping up seconds after the press would take the keyboard from whatever the user
/// went on to do.
const STALE_PRESS: Duration = Duration::from_millis(500);

/// Whether a bar shows a built-in module.
fn has_module(bar: &Bar, module: Module) -> bool {
    bar_modules(bar.kind, &bar.config).any(|m| *m == ModuleRef::Builtin(module))
}

impl App {
    /// Whether a bar shows a topic: one of its modules, or a flyout open on it.
    pub(super) fn shows(&self, bar: &Bar, topic: Topic) -> bool {
        bar_topics(bar.kind, &bar.config).contains(&topic)
            || self.flyouts.watches.watching(bar.id, Watched::Core(topic))
    }

    // --- Flyouts ------------------------------------------------------------------------------------

    /// A flyout's request. The ones about the bars themselves are answered here, the providers'
    /// by `Flyouts` (`None`: the answer comes later).
    pub(super) fn flyout(
        &mut self,
        index: usize,
        id: u64,
        request: Request,
    ) -> Option<Result<Value, BarError>> {
        let bar = self.bars[index].id;
        let done = Some(Ok(Value::Null));
        match request {
            Request::Watch { topic } => {
                let Some(what) = Watched::parse(&topic) else {
                    return Some(Err(BarError::invalid(format!("watch: no topic {topic:?}"))));
                };
                if self.flyouts.watches.add(bar, what) {
                    self.flyouts.watched(what);
                    if let Watched::Core(topic) = what {
                        self.sync_topics();
                        self.sent.remove(&(bar, topic));
                        self.emit(topic);
                    }
                }
                done
            }
            Request::Unwatch { topic } => {
                if let Some(what) = Watched::parse(&topic) {
                    self.flyouts.watches.remove(bar, what);
                    if matches!(what, Watched::Core(_)) {
                        self.sync_topics();
                    }
                }
                done
            }
            Request::Room { extent } => {
                self.bars[index].set_room(extent);
                done
            }
            Request::Keyboard { on, hand_back } => {
                self.take_keyboard(index, on, hand_back);
                done
            }
            // The shell's HSHELL_LANGUAGE (or the next foreground change) updates the module.
            Request::SetLayout { id: layout } => Some(match self.foreground {
                Some(window) => lang::switch(window, &layout)
                    .map(|()| Value::Null)
                    .map_err(BarError::unavailable),
                None => Err(BarError::unavailable("no app in front")),
            }),
            Request::LauncherPin {
                path,
                name,
                index: at,
            } => Some(self.edit_launcher_pins(|pins| {
                let pinned = Pinned { path, name };
                let found = pins
                    .iter()
                    .position(|p| p.path.eq_ignore_ascii_case(&pinned.path));
                let pinned = match found {
                    Some(found) => pins.remove(found),
                    None => pinned,
                };
                let at = at.unwrap_or(pins.len()).min(pins.len());
                pins.insert(at, pinned);
            })),
            Request::LauncherUnpin { path } => {
                Some(self.edit_launcher_pins(|pins| {
                    pins.retain(|p| !p.path.eq_ignore_ascii_case(&path))
                }))
            }
            other => self.flyouts.call(bar, id, other),
        }
    }

    pub(super) fn on_flyout(&mut self, done: Done) {
        let post_to = |bars: &[Bar], id: u32, message: &str| {
            if let Some(bar) = bars.iter().find(|b| b.id == id && b.ready) {
                bar.post(message);
            }
        };
        match done {
            Done::Reply {
                bar,
                page,
                id,
                result,
            } => {
                if !self.flyouts.current_page(bar, page) {
                    return; // the page reloaded: `id` may be one of its new requests
                }
                let message = match result {
                    Ok(result) => json!({ "id": id, "result": result }),
                    Err(error) => {
                        log::info!("flyout request {id}: {}", error.message);
                        json!({ "id": id, "error": error })
                    }
                };
                post_to(&self.bars, bar, &message.to_string());
            }
            Done::Event {
                topic: Watched::Core(Topic::Notifications),
                data,
            } => {
                self.notification_count = Some(data["count"].as_u64().map(|c| c as usize));
                self.emit(Topic::Notifications);
            }
            Done::Event { topic, data } => {
                let message = json!({ "event": topic.event(), "data": data }).to_string();
                for bar in self.flyouts.watches.bars(topic).collect::<Vec<_>>() {
                    post_to(&self.bars, bar, &message);
                }
            }
            Done::WinKey(at, target) => self.open_from_key(at, target),
            Done::Index(apps) => {
                let (value, replies) = self.flyouts.index_built(apps);
                for (bar, id, result) in replies {
                    post_to(
                        &self.bars,
                        bar,
                        &json!({ "id": id, "result": result }).to_string(),
                    );
                }
                let message = json!({ "event": Watched::Apps.event(), "data": value }).to_string();
                for bar in self.flyouts.watches.bars(Watched::Apps).collect::<Vec<_>>() {
                    post_to(&self.bars, bar, &message);
                }
            }
            Done::Changed(Watched::Core(Topic::Language)) => {
                self.layouts = None; // a layout may have been added or removed
                self.emit(Topic::Language);
            }
            Done::Changed(what) => self.flyouts.changed(what),
        }
    }

    /// The Windows key hook runs while the launcher replaces Start (`launcher.winKey` with the
    /// native taskbar hidden) and opens it while some bar can show it.
    pub(super) fn sync_winkey(&mut self) {
        let wanted = self.config.launcher.win_key
            && self.wanted_taskbar().mode == TaskbarMode::Hide
            && !self.bars.is_empty();
        match (wanted, self.winkey.is_some()) {
            (true, false) => {
                // From now on `refresh_fullscreen` keeps it current.
                WinKey::set_fullscreen(mypaper_desktop::fullscreen_app_monitor().is_some());
                self.winkey = WinKey::start(self.waker.clone(), self.broadcast);
            }
            (false, true) => self.winkey = None,
            _ => {}
        }
        WinKey::set_enabled(
            wanted && !self.asleep && self.bars.iter().any(|b| b.ready && !b.hidden),
        );
    }

    /// A Windows key shortcut (at `at`): `target`'s flyout opens on the monitor the user works on
    /// (the app in front's), on a bar with its module (`start`, `notifications`, `quick`) if
    /// there's one, and takes the keyboard. With no bar to show it, the native panel opens instead.
    pub(super) fn open_from_key(&mut self, at: Instant, target: Target) {
        if at.elapsed() > STALE_PRESS {
            return log::info!(
                "launcher: a Windows key press {:?} old; not opening",
                at.elapsed()
            );
        }
        let module = match target {
            Target::Launcher => Module::Start,
            Target::Notifications => Module::Notifications,
            Target::Quick => Module::Quick,
        };
        let mut rect = windows::Win32::Foundation::RECT::default();
        let center = self.foreground.and_then(|window| {
            // SAFETY: a read-only query; `rect` outlives it.
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::GetWindowRect(
                    system::hwnd(window),
                    &mut rect,
                )
            }
            .ok()
            .map(|()| ((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2))
        });
        let index = self
            .bars
            .iter()
            .enumerate()
            .filter(|(_, b)| b.ready && !b.hidden)
            .max_by_key(|(_, b)| {
                let here = center.is_some_and(|(x, y)| b.monitor.rect.contains_point(x, y));
                let has = has_module(b, module);
                (here, has, b.kind == BarKind::Top, b.monitor.primary)
            })
            .map(|(index, _)| index);
        let Some(index) = index else {
            return match target {
                Target::Launcher => system::start_menu(),
                Target::Notifications => system::action_center(Panel::Notifications),
                Target::Quick => system::action_center(Panel::Quick),
            };
        };
        log::debug!(
            "Windows key: opening the {} flyout on bar {index}",
            target.flyout()
        );
        self.take_keyboard(index, true, false);
        self.post_event(index, "open", &json!({ "flyout": target.flyout() }));
    }

    /// Makes the bar the foreground window with the keyboard in its page (a flyout that types),
    /// or lets it go: with `hand_back` (the page saw no press close it) the app that was in
    /// front gets the foreground back. A press closed it otherwise: that click keeps the bar, or
    /// whatever it hit, in front.
    pub(super) fn take_keyboard(&mut self, index: usize, on: bool, hand_back: bool) {
        let bar = &self.bars[index];
        if on {
            self.keyboard = Some(bar.id);
            // A click already gave the bar the foreground and its page the focus; moving the focus
            // again would blur the page (and close the flyout asking).
            // SAFETY: a read-only query.
            if unsafe { GetForegroundWindow() } == bar.hwnd() {
                return;
            }
            // SAFETY: our own window; Windows refuses it without the foreground rights a click
            // or the key press gave us.
            if !unsafe { SetForegroundWindow(bar.hwnd()) }.as_bool() {
                log::info!("Windows refused the foreground to the {:?} bar", bar.kind);
            }
            if let Some(web) = &bar.web {
                web.focus();
            }
        } else if self.keyboard == Some(bar.id) {
            self.keyboard = None;
            // SAFETY: read-only queries.
            let ours = unsafe {
                let mut pid = 0;
                GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
                pid == std::process::id()
            };
            // An app that took the foreground (one just launched) is left alone.
            if hand_back && ours {
                self.hand_back(None);
            }
        }
    }

    /// Edits the launcher's pins (`launcher.pinned`; until the first edit, the dock's) and tells
    /// every bar.
    pub(super) fn edit_launcher_pins(
        &mut self,
        change: impl FnOnce(&mut Vec<Pinned>),
    ) -> Result<Value, BarError> {
        let mut pins = (self.config.launcher.pinned.clone())
            .or_else(|| self.config.dock.pinned.clone())
            .unwrap_or_default();
        change(&mut pins);
        let value = serde_json::to_value(&pins).map_err(|e| BarError::internal(e.to_string()))?;
        config::edit(&self.config_path, |root| {
            config::set(root, "launcher", "pinned", value.clone())
        })
        .map_err(|e| BarError::internal(format!("{e:#}")))?;
        self.config.launcher.pinned = Some(pins);
        self.broadcast_event("launcherPins", |_| value.clone());
        Ok(Value::Null)
    }

    // --- The tray -----------------------------------------------------------------------------------

    /// The tray host runs while some bar is configured with a tray module and the native
    /// taskbar is replaced (`taskbar.mode: hide`, or `tray.host`). Configuration, not what's on
    /// screen right now: each start and stop makes every app re-add its icons. Otherwise UI
    /// Automation reads Explorer's tray, while a tray module is on screen.
    pub(super) fn sync_tray(&mut self) {
        let configured = self.bars.iter().any(|b| has_module(b, Module::Tray));
        let replaced = self.wanted_taskbar().mode == TaskbarMode::Hide || self.config.tray.host;
        let host = configured && replaced;
        if host != self.tray_host.active() {
            // The other source reports its own list.
            self.tray_items.clear();
        }
        self.tray
            .set_active(self.active.contains(&Topic::Tray) && !host);
        self.tray_host.set_active(host);
        self.place_tray();
    }

    /// Tells the host where the tray is: the band of a bar showing it (the primary monitor's,
    /// and one on screen, first). Apps put their popups there.
    pub(super) fn place_tray(&mut self) {
        let band = self
            .bars
            .iter()
            .filter(|b| has_module(b, Module::Tray))
            .max_by_key(|b| (!b.hidden, b.monitor.primary))
            .map(|bar| tray_host::Band {
                rect: [bar.band.x, bar.band.y, bar.band.width, bar.band.height],
                edge: bar.geometry.edge.abe(),
            });
        self.tray_host.place(band);
    }

    pub(super) fn click_tray(
        &mut self,
        index: usize,
        id: String,
        button: Button,
        rect: Option<[f64; 4]>,
    ) {
        if !self.tray_host.active() {
            return self.tray.click(id, button);
        }
        let bar = &self.bars[index];
        let scale = bar.scale();
        let anchor = rect.map(|[x, y, w, h]| {
            let px = |v: f64| (v * scale).round() as i32;
            [bar.rect.x + px(x), bar.rect.y + px(y), px(w), px(h)]
        });
        let mut cursor = POINT::default();
        // SAFETY: `cursor` outlives the call.
        let _ = unsafe { GetCursorPos(&mut cursor) };
        let click = match button {
            Button::Left => Click::Left,
            Button::Right => Click::Right,
            Button::Middle => Click::Middle,
            Button::Double => Click::Double,
        };
        self.tray_host
            .click(id, click, (cursor.x, cursor.y), anchor);
    }
}
