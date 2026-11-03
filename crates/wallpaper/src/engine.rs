//! The frame loop policy: when to draw, when to sleep, when to reload.
//!
//! The engine never draws a frame nobody can see or that would look the
//! same as the last one: covered screens are skipped, static sources draw
//! once, animated ones are paced by their own fps cap, and the clock that
//! drives animation stops while everything is hidden.

use std::time::{Duration, Instant};

use anyhow::Result;
use mypaper_desktop::{EXIT_RESTART, Rect, WindowEvents, covered_monitors, monitors};
use mypaper_theme::LoadedTheme;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::gpu::{Gpu, Screen};
use crate::sources::{self, Frame, Redraw, Source};
use crate::window::Window;

/// Coalesces the burst of file events an editor save produces.
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(80);
/// Coalesces window-move storms before re-checking what is covered.
const COVER_DEBOUNCE: Duration = Duration::from_millis(100);
/// Slack on the longest clock step between ticks, so a stall (or system
/// sleep) never jumps the clock ahead. The step may also cover the sleep the
/// engine itself planned (see `Engine::step_limit`), or a source scheduled
/// with `Redraw::At` would wake with the clock barely moved.
const MAX_STEP: Duration = Duration::from_millis(100);

pub enum Flow {
    /// Sleep until this deadline (or the next message, if `None`).
    Wait(Option<Instant>),
    Exit(u8),
}

pub struct Engine {
    theme: LoadedTheme,
    gpu: Gpu,
    source: Box<dyn Source>,
    screens: Vec<Screen>,
    /// Monitor rects for desktop mode; `None` in dev (windowed) mode.
    monitors: Option<Vec<Rect>>,
    /// Animation clock: advances only while some screen is visible.
    clock: Duration,
    last_tick: Option<Instant>,
    /// Longest clock step the next tick may take: the planned sleep plus `MAX_STEP`.
    step_limit: Duration,
    next_frame: Instant,
    frame: u32,
    reload_at: Option<Instant>,
    recheck_at: Option<Instant>,
    /// Externally requested pause (battery saver, session locked, user).
    held: bool,
}

impl Engine {
    pub fn new(
        theme: LoadedTheme,
        windows: Vec<Window>,
        monitors: Option<Vec<Rect>>,
    ) -> Result<Self> {
        let (gpu, surfaces) = Gpu::new(&windows)?;
        let screens = surfaces
            .into_iter()
            .zip(windows)
            .map(|(surface, window)| Screen::new(&gpu, surface, window))
            .collect::<Result<_>>()?;
        let source = load_or_broken(&gpu, &theme);
        Ok(Self {
            theme,
            gpu,
            source,
            screens,
            monitors,
            clock: Duration::ZERO,
            last_tick: None,
            step_limit: MAX_STEP,
            next_frame: Instant::now(),
            frame: 0,
            reload_at: None,
            recheck_at: None,
            held: false,
        })
    }

    pub fn schedule_reload(&mut self) {
        self.reload_at = Some(Instant::now() + RELOAD_DEBOUNCE);
    }

    /// Pauses or resumes every screen regardless of coverage.
    pub fn hold(&mut self, held: bool) {
        if self.held != held {
            log::info!("{}", if held { "held (paused)" } else { "released" });
            self.held = held;
            self.mark_dirty();
        }
    }

    /// One turn of the loop: react to changes, draw what is due, and say
    /// how long the thread may sleep.
    pub fn tick(&mut self, events: Option<&WindowEvents>) -> Flow {
        let now = Instant::now();

        if events.is_some_and(WindowEvents::take_changed) {
            self.recheck_at.get_or_insert(now + COVER_DEBOUNCE);
        }
        if self.recheck_at.is_some_and(|t| now >= t) {
            self.recheck_at = None;
            if let Some(known) = &self.monitors {
                if monitors() != *known {
                    log::info!("monitor layout changed; restarting");
                    return Flow::Exit(EXIT_RESTART);
                }
                for (screen, covered) in self.screens.iter_mut().zip(covered_monitors(known)) {
                    if screen.paused != covered {
                        let (w, h) = screen.size();
                        let verb = if covered { "pausing" } else { "resuming" };
                        log::info!("{verb} {w}x{h} screen");
                        screen.dirty |= !covered;
                    }
                    screen.paused = covered;
                }
            }
        }
        if self.reload_at.is_some_and(|t| now >= t) {
            self.reload_at = None;
            self.reload();
        }
        if self.monitors.is_none() {
            for screen in &mut self.screens {
                let minimized = screen.window.minimized();
                screen.dirty |= screen.paused && !minimized;
                screen.paused = minimized;
                screen.sync_size(&self.gpu);
            }
        }

        let active = !self.held && self.screens.iter().any(|s| !s.paused);
        if !active {
            self.last_tick = None; // the clock stops while nothing is visible
            return Flow::Wait(self.deadline(None));
        }
        self.clock += self
            .last_tick
            .map_or(Duration::ZERO, |t| (now - t).min(self.step_limit));
        self.last_tick = Some(now);

        self.source.update(&self.gpu, self.clock);
        let redraw = self.source.redraw(self.clock);
        let due = match redraw {
            Redraw::Every(_) => now >= self.next_frame,
            Redraw::At(t) => self.clock >= t,
            Redraw::OnChange => false,
        };
        if due || self.screens.iter().any(|s| s.dirty && !s.paused) {
            self.render(due);
        }

        let frame_deadline = match redraw {
            Redraw::Every(interval) => {
                if due {
                    self.next_frame = (self.next_frame + interval).max(now);
                }
                Some(self.next_frame)
            }
            Redraw::At(t) => Some(now + t.saturating_sub(self.clock)),
            Redraw::OnChange => None,
        };
        self.step_limit =
            MAX_STEP + frame_deadline.map_or(Duration::ZERO, |t| t.saturating_duration_since(now));
        Flow::Wait(self.deadline(frame_deadline))
    }

    fn deadline(&self, frame: Option<Instant>) -> Option<Instant> {
        [frame, self.reload_at, self.recheck_at]
            .into_iter()
            .flatten()
            .min()
    }

    /// Draws visible screens: all of them if a frame is `due`, otherwise
    /// only the dirty ones.
    fn render(&mut self, due: bool) {
        self.frame = self.frame.wrapping_add(1);
        let mut cursor = POINT::default();
        // SAFETY: `cursor` outlives the call.
        let _ = unsafe { GetCursorPos(&mut cursor) };

        for (index, screen) in self.screens.iter_mut().enumerate() {
            if screen.paused || !(due || screen.dirty) {
                continue;
            }
            let Some(texture) = screen.acquire(&self.gpu) else {
                continue;
            };
            screen.dirty = false;
            let mut mouse = cursor;
            // SAFETY: valid window; `mouse` outlives the call.
            let _ = unsafe { ScreenToClient(screen.window.0, &mut mouse) };

            let view = texture.texture.create_view(&Default::default());
            let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
            self.source.render(
                &self.gpu,
                Frame {
                    screen: index,
                    size: screen.size(),
                    mouse: [mouse.x as f32, mouse.y as f32],
                    clock: self.clock,
                    frame: self.frame,
                    view: &view,
                    encoder: &mut encoder,
                },
            );
            self.gpu.queue.submit([encoder.finish()]);
            self.gpu.queue.present(texture);
        }
    }

    fn reload(&mut self) {
        let theme = match LoadedTheme::load(&self.theme.dir) {
            Ok(theme) => theme,
            Err(e) => return log::error!("{e:#}\n(keeping the current wallpaper)"),
        };
        let started = Instant::now();
        match sources::load(&self.gpu, &theme) {
            Ok(source) => {
                log::info!("{} reloaded in {:.1?}", theme.theme.name, started.elapsed());
                self.source = source;
                self.theme = theme;
                self.next_frame = Instant::now();
                self.mark_dirty();
            }
            Err(e) => log::error!("{e:#}\n(keeping the current wallpaper)"),
        }
    }

    fn mark_dirty(&mut self) {
        for screen in &mut self.screens {
            screen.dirty = true;
        }
    }
}

/// A theme that fails at startup shows the red "broken" screen and keeps
/// watching for a fix, instead of exiting.
fn load_or_broken(gpu: &Gpu, theme: &LoadedTheme) -> Box<dyn Source> {
    let started = Instant::now();
    match sources::load(gpu, theme) {
        Ok(source) => {
            log::info!("{} ready in {:.1?}", theme.theme.name, started.elapsed());
            source
        }
        Err(e) => {
            log::error!("{e:#}");
            Box::new(sources::Broken)
        }
    }
}
