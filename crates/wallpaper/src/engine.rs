//! The frame loop policy: when to draw, when to sleep, when to reload.
//!
//! The engine never draws a frame nobody can see or that would look the
//! same as the last one: covered screens are skipped, static sources draw
//! once, animated ones are paced by their own fps cap, and the clock that
//! drives animation stops while everything is hidden.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use mypaper_config::Config;
use mypaper_desktop::{
    EXIT_RESTART, Rect, WindowEvents, covered_monitors, fullscreen_app_in_front, monitors, startup,
};
use mypaper_theme::LoadedTheme;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::backdrop::{Backdrop, Draw};
use crate::glass_pass::GlassPass;
use crate::gpu::{Gpu, Opening, Screen};
use crate::hold::{Fact, HoldReasons, Holds};
use crate::palette;
use crate::snapshot::{Snapshot, Target};
use crate::sources::{self, Frame, Redraw, Source, Visibility};
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
    /// Per screen: what `source` was last told via `Source::set_visible`.
    shown: Vec<Visibility>,
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
    /// Pauses every screen regardless of coverage. Derived from `holds`.
    held: bool,
    /// Settings and system conditions that can hold the wallpaper.
    holds: Holds,
    /// The reasons behind the current `held`, for logging changes.
    reasons: HoldReasons,
    /// The loaded theme's palette has not been written yet (once per load).
    palette_due: bool,
    /// Publishes each screen's blurred frame for the bars (see `backdrop`).
    backdrop: Backdrop,
    /// Sets a still of the theme as the Windows wallpaper (desktop mode, `static_snapshot`).
    snapshot: Snapshot,
    /// Liquid glass under the widgets' (and bars') plates, when they ask for it.
    glass: GlassPass,
}

impl Engine {
    /// Takes the device `gpu` is opening (see [`Opening`]) and shows `theme` in `windows`.
    pub fn new(
        theme: LoadedTheme,
        gpu: Opening,
        windows: Vec<Window>,
        monitors: Option<Vec<Rect>>,
    ) -> Result<Self> {
        let (gpu, surfaces) = Gpu::new(gpu, &windows)?;
        startup::ready("wallpaper", "device_ms");
        let mut screens: Vec<Screen> = surfaces
            .into_iter()
            .zip(windows)
            .map(|(surface, window)| Screen::new(&gpu, surface, window))
            .collect::<Result<_>>()?;
        // Only what can be seen is drawn, from the first frame on: a screen a window already
        // covers starts paused, and makes its swapchain (the costliest part of a first frame,
        // 50-150 ms here) only once it's uncovered.
        if let Some(known) = &monitors {
            for (screen, covered) in screens.iter_mut().zip(covered_monitors(known)) {
                if covered {
                    let (w, h) = screen.size();
                    log::info!("pausing {w}x{h} screen");
                    screen.paused = true;
                }
            }
        }
        let hwnds: Vec<HWND> = screens.iter().map(|s| s.window.0).collect();
        let (source, loaded) = load_or_broken(&gpu, &theme, &hwnds);
        startup::ready("wallpaper", "source_ms");
        let mut snapshot = Snapshot::new();
        snapshot.theme_loaded(loaded);
        let backdrop = Backdrop::new(monitors.as_deref(), |i| {
            source.native(i) || !screens[i].copy_src
        });
        let glass = GlassPass::new(monitors.as_deref());
        Ok(Self {
            theme,
            gpu,
            source,
            shown: vec![Visibility::Live; screens.len()],
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
            holds: Holds::default(),
            reasons: HoldReasons::default(),
            palette_due: true,
            backdrop,
            snapshot,
            glass,
        })
    }

    /// The registered message that says a glass plates file changed.
    pub fn glass_message(&self) -> u32 {
        self.glass.message
    }

    /// Re-reads the glass plates (their writer posted `mypaper.glass`).
    pub fn glass_changed(&mut self) {
        self.glass.reload();
    }

    /// Follows `static_snapshot`: on, the theme's first frame becomes the Windows wallpaper;
    /// off, the user's own wallpaper is put back. Does nothing in the dev window, which must
    /// not touch the real desktop.
    pub fn set_static_snapshot(&mut self, on: bool) {
        if self.monitors.is_some() && self.snapshot.set_enabled(on) {
            self.mark_dirty();
        }
    }

    pub fn schedule_reload(&mut self) {
        self.reload_at = Some(Instant::now() + RELOAD_DEBOUNCE);
    }

    /// The folder of the theme being shown.
    pub fn theme_dir(&self) -> &Path {
        &self.theme.dir
    }

    /// Takes the pause settings from `config` and re-evaluates the hold.
    pub fn apply_config(&mut self, config: &Config) {
        self.holds.apply(config);
        self.refresh_fullscreen();
        self.sync_hold();
    }

    /// Records a system condition (battery, display, lock) and re-evaluates the hold.
    pub fn set_fact(&mut self, fact: Fact, value: bool) {
        self.holds.set(fact, value);
        self.sync_hold();
    }

    /// Re-checks for a fullscreen app in front. Only the desktop wallpaper
    /// yields to one; the dev window is a normal window.
    fn refresh_fullscreen(&mut self) {
        let fullscreen =
            self.monitors.is_some() && self.holds.wants_fullscreen() && fullscreen_app_in_front();
        self.holds.set_fullscreen(fullscreen);
    }

    /// Combines every hold reason into [`hold`](Self::hold), logging changes.
    fn sync_hold(&mut self) {
        let reasons = self.holds.reasons();
        if reasons == self.reasons {
            return;
        }
        if reasons.is_empty() {
            log::info!("released (was held: {})", self.reasons);
        } else {
            log::info!("held: {reasons}");
        }
        self.reasons = reasons;
        self.hold(!reasons.is_empty());
    }

    /// Pauses or resumes every screen regardless of coverage.
    fn hold(&mut self, held: bool) {
        if self.held != held {
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
            // Same trigger: a game going fullscreen shows up as a window event.
            self.refresh_fullscreen();
            self.sync_hold();
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

        self.sync_visibility();
        self.glass.advance(now);
        for (index, screen) in self.screens.iter_mut().enumerate() {
            screen.dirty |= self.source.take_dirty(index);
            screen.dirty |= self.glass.take_dirty(index);
        }
        let active = !self.held && self.screens.iter().any(|s| !s.paused);
        if !active {
            self.last_tick = None; // the clock stops while nothing is visible
            // Held, but an uncovered screen is still seen (e.g. the second
            // monitor while a game is fullscreen): give it one still frame of
            // the current wallpaper rather than whatever was there before.
            if self.screens.iter().any(|s| s.dirty && !s.paused) {
                self.render(false);
            }
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

    /// Tells the source about screens whose visibility changed since the last
    /// call, so it can suspend what it runs for them.
    fn sync_visibility(&mut self) {
        for (index, screen) in self.screens.iter().enumerate() {
            let visibility = Visibility::of(self.held, screen.paused);
            if self.shown[index] != visibility {
                self.shown[index] = visibility;
                self.source
                    .set_visibility(index, screen.window.0, visibility);
            }
        }
    }

    fn deadline(&self, frame: Option<Instant>) -> Option<Instant> {
        [
            frame,
            self.reload_at,
            self.recheck_at,
            self.glass.deadline(),
        ]
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
        let mut drew = false;
        let animated = !matches!(self.source.redraw(self.clock), Redraw::OnChange);
        let mut native = false;
        for (index, screen) in self.screens.iter_mut().enumerate() {
            if screen.paused || !(due || screen.dirty) {
                continue;
            }
            if self.source.native(index) {
                // The source owns what is on screen: nothing to acquire or present.
                screen.dirty = false;
                native = true;
                self.source.layout(index, screen.window.0, screen.size());
                if self.palette_due {
                    startup::ready("wallpaper", "first_frame_ms");
                }
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
            let publish = self.backdrop.encode(
                &self.gpu,
                Draw {
                    screen: index,
                    size: screen.size(),
                    texture: &texture.texture,
                    copy_src: screen.copy_src,
                    held: self.held,
                    animated,
                },
                &mut encoder,
            );
            // After the backdrop's copy, so the bars' blurred wallpaper has no plates in it.
            self.glass.encode(
                &self.gpu,
                index,
                screen.size(),
                &texture.texture,
                screen.copy_src,
                &mut encoder,
            );
            self.gpu.queue.submit([encoder.finish()]);
            self.gpu.queue.present(texture);
            if publish {
                self.backdrop.finish(&self.gpu, index);
            }
            self.source.presented(index);
            drew = true;
        }

        // First frame of a newly loaded theme: publish its palette, once.
        if drew && std::mem::take(&mut self.palette_due) {
            startup::ready("wallpaper", "first_frame_ms");
            let size = self.screens.first().map_or((1, 1), Screen::size);
            palette::publish(
                &self.gpu,
                self.source.as_mut(),
                &self.theme.dir,
                size,
                self.clock,
                self.frame,
            );
        }

        // Once per theme load, too: the same frame, full size, for the Windows wallpaper (a
        // native source has none to give, which the snapshot answers by restoring the original).
        if (drew || native) && self.snapshot.due() {
            let monitors = self.monitors.iter().flatten();
            let targets: Vec<Target> = monitors
                .zip(&self.screens)
                .map(|(rect, screen)| Target {
                    rect: *rect,
                    size: screen.size(),
                    mouse: [(cursor.x - rect.x) as f32, (cursor.y - rect.y) as f32],
                })
                .collect();
            self.snapshot.capture(
                &self.gpu,
                self.source.as_mut(),
                &self.theme.dir,
                &targets,
                self.clock,
                self.frame,
            );
        }
    }

    /// Reloads the current theme after its files changed.
    fn reload(&mut self) {
        let dir = self.theme.dir.clone();
        if let Err(e) = self.switch_theme(&dir) {
            log::error!("{e:#}\n(keeping the current wallpaper)");
        }
    }

    /// Loads the theme in `dir` and swaps it in, in-process. On failure
    /// nothing changes and the current theme keeps running.
    pub fn switch_theme(&mut self, dir: &Path) -> Result<()> {
        let theme = LoadedTheme::load(dir)?;
        let started = Instant::now();
        let hwnds: Vec<HWND> = self.screens.iter().map(|s| s.window.0).collect();
        let source = sources::load(&self.gpu, &theme, &hwnds)?;
        log::info!("{} loaded in {:.1?}", theme.theme.name, started.elapsed());
        self.source = source;
        self.theme = theme;
        self.palette_due = true;
        self.backdrop
            .reset(|i| self.source.native(i) || !self.screens[i].copy_src);
        self.snapshot.theme_loaded(true);
        self.next_frame = Instant::now();
        // A new source starts with every screen visible.
        self.shown.fill(Visibility::Live);
        self.sync_visibility();
        self.mark_dirty();
        Ok(())
    }

    fn mark_dirty(&mut self) {
        for screen in &mut self.screens {
            screen.dirty = true;
        }
    }
}

/// A theme that fails at startup shows the red "broken" screen and keeps
/// watching for a fix, instead of exiting. Also says whether the theme loaded.
fn load_or_broken(gpu: &Gpu, theme: &LoadedTheme, screens: &[HWND]) -> (Box<dyn Source>, bool) {
    let started = Instant::now();
    match sources::load(gpu, theme, screens) {
        Ok(source) => {
            log::info!("{} ready in {:.1?}", theme.theme.name, started.elapsed());
            (source, true)
        }
        Err(e) => {
            log::error!("{e:#}");
            (Box::new(sources::Broken), false)
        }
    }
}
