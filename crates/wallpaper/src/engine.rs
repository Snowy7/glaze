//! The frame loop policy: when to draw, when to sleep, when to reload.
//!
//! The engine never draws a frame nobody can see or that would look the
//! same as the last one: covered screens are skipped, static sources draw
//! once, animated ones are paced by their own fps cap, and the clock that
//! drives animation stops while everything is hidden.

use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::Result;
use mypaper_config::{Config, EngineConfig, RenderSettings};
use mypaper_desktop::{
    EXIT_RESTART, Rect, WindowEvents, covered_monitors, fullscreen_app_in_front, monitors, startup,
};
use mypaper_theme::LoadedTheme;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::backdrop::{Backdrop, Draw};
use crate::glass_pass::GlassPass;
use crate::gpu::{Gpu, KeptFrame, Opening, Screen};
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
    /// Studio's preview requests (D3): idle unless Studio asks.
    preview: crate::preview::Preview,
    timing: Option<crate::timing::Timing>,
    renderer_config: EngineConfig,
    render_settings: RenderSettings,
    restart_due: bool,
}

impl Engine {
    /// Takes the device `gpu` is opening (see [`Opening`]) and shows `theme` in `windows`.
    pub fn new(
        theme: LoadedTheme,
        gpu: Opening,
        windows: Vec<Window>,
        monitors: Option<Vec<Rect>>,
        renderer_config: EngineConfig,
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
        let backdrop = Backdrop::new(monitors.as_deref(), |i| source.native(i));
        let glass = GlassPass::new(monitors.as_deref())?;
        let timing = crate::timing::Timing::new(&gpu)?;
        let id = theme
            .dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let render_settings = renderer_config.resolve(id, theme.theme.engine);
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
            preview: crate::preview::Preview::new(),
            timing,
            renderer_config,
            render_settings,
            restart_due: false,
        })
    }

    /// The device the glass overlays draw with: the engine's own (plan Rule 1).
    pub fn overlay_gpu(&self) -> crate::overlay::Gpu {
        let gpu = &self.gpu;
        let (adapter, device, queue) = (gpu.adapter.clone(), gpu.device.clone(), gpu.queue.clone());
        crate::overlay::Gpu {
            adapter,
            device,
            queue,
        }
    }

    /// Named plate event plus the current writer's exit handle, both owned by GlassPass.
    pub fn glass_handles(&self) -> Vec<windows::Win32::Foundation::HANDLE> {
        let mut handles = self.glass.wait_handles();
        handles.extend(self.preview.handle());
        handles
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
        self.apply_renderer_config(&config.engine);
        self.holds.apply(config);
        self.refresh_fullscreen();
        self.sync_hold();
    }

    /// FPS caps change in process. A GPU preference change asks the core supervisor to restart
    /// just this engine, releasing the old device before the new one is opened.
    pub fn apply_renderer_config(&mut self, config: &EngineConfig) {
        self.renderer_config = config.clone();
        self.refresh_renderer();
    }
    fn refresh_renderer(&mut self) {
        let id = self
            .theme
            .dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let next = self.renderer_config.resolve(id, self.theme.theme.engine);
        if next != self.render_settings {
            log::info!("renderer settings: {next:?}");
            self.next_frame = Instant::now();
        }
        self.render_settings = next;
        self.restart_due = next.gpu != self.gpu.preference;
    }
    fn redraw(&self) -> Redraw {
        cap_redraw(self.source.redraw(self.clock), self.render_settings.fps)
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

    /// One turn of the loop: react to changes, draw what is due, answer Studio's preview, and
    /// say how long the thread may sleep.
    pub fn tick(&mut self, events: Option<&WindowEvents>) -> Flow {
        let flow = self.step(events);
        // After drawing, so a preview waiting for a frame gets the one just drawn. Any screen's
        // will do: a covered first screen has none.
        let kept = self
            .screens
            .iter()
            .filter_map(|s| s.kept.as_ref())
            .find(|k| k.drawn.get().is_some());
        self.preview.serve(&self.gpu, kept.map(|k| &k.texture));
        flow
    }

    fn step(&mut self, events: Option<&WindowEvents>) -> Flow {
        if self.restart_due {
            return Flow::Exit(EXIT_RESTART);
        }
        let now = Instant::now();
        if let Some(timing) = &mut self.timing {
            timing.collect(&self.gpu);
        }

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
        self.glass.reload();
        for (index, screen) in self.screens.iter_mut().enumerate() {
            screen.dirty |= self.source.take_dirty(index);
            screen.glass_dirty |= self.glass.take_dirty(index);
        }
        let active = !self.held && self.screens.iter().any(|s| !s.paused);
        if !active {
            self.last_tick = None; // the clock stops while nothing is visible
            // Held, but an uncovered screen is still seen (e.g. the second
            // monitor while a game is fullscreen): give it one still frame of
            // the current wallpaper rather than whatever was there before.
            if self
                .screens
                .iter()
                .any(|s| (s.dirty || s.glass_dirty) && !s.paused)
            {
                self.render(false);
            }
            return Flow::Wait(self.deadline(None));
        }
        self.clock += self
            .last_tick
            .map_or(Duration::ZERO, |t| (now - t).min(self.step_limit));
        self.last_tick = Some(now);

        self.source.update(&self.gpu, self.clock);
        let redraw = self.redraw();
        let due = match redraw {
            Redraw::Every(_) => now >= self.next_frame,
            Redraw::At(t) => self.clock >= t,
            Redraw::OnChange => false,
        };
        if due
            || self
                .screens
                .iter()
                .any(|s| (s.dirty || s.glass_dirty) && !s.paused)
        {
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
            // A glass spring (materialize, a material fade) runs: present pacing sets the rate.
            self.glass
                .animating()
                .then(|| Instant::now() + Duration::from_millis(4)),
            self.reload_at,
            self.recheck_at,
            self.timing
                .as_ref()
                .filter(|t| t.pending())
                .map(|_| Instant::now() + Duration::from_millis(2)),
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
        let animated = !matches!(self.redraw(), Redraw::OnChange);
        let mut native = false;
        self.assign_kept_frames();
        for (index, screen) in self.screens.iter_mut().enumerate() {
            if screen.paused || !(due || screen.dirty || screen.glass_dirty) {
                continue;
            }
            if self.source.native(index) {
                // The source owns what is on screen: nothing to acquire or present.
                screen.dirty = false;
                screen.glass_dirty = false;
                screen.kept = None;
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
            // Acquisition can wait for DWM/GPU availability. Late-latch the newest shell
            // snapshot after that wait, so a display-rate plate does not inherit its delay.
            self.glass.reload();
            self.glass.take_dirty(index);
            let kept = screen.kept.clone().expect("kept frame assigned");
            let wallpaper_due = due || screen.dirty || kept.drawn.get().is_none();
            // A kept frame shared with a screen drawn earlier in this frame already holds it.
            let draw_wallpaper = wallpaper_due && kept.drawn.get() != Some(self.frame);
            screen.dirty = false;
            screen.glass_dirty = false;
            let mut mouse = cursor;
            // SAFETY: valid window; `mouse` outlives the call.
            let _ = unsafe { ScreenToClient(screen.window.0, &mut mouse) };

            let view = texture.texture.create_view(&Default::default());
            let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
            let size = kept.size();
            let timing_slot = self.timing.as_mut().and_then(|timing| {
                timing.begin(
                    &self.gpu,
                    &mut encoder,
                    crate::timing::Meta {
                        screen: index,
                        frame: self.frame,
                        wallpaper: wallpaper_due,
                        pass: draw_wallpaper,
                        sequence: self.glass.sequence,
                        published_qpc: self.glass.published_qpc,
                        ..Default::default()
                    },
                )
            });
            if draw_wallpaper {
                kept.drawn.set(Some(self.frame));
                self.source.render(
                    &self.gpu,
                    Frame {
                        screen: index,
                        size,
                        mouse: [
                            mouse.x as f32 * size.0 as f32 / screen.size().0 as f32,
                            mouse.y as f32 * size.1 as f32 / screen.size().1 as f32,
                        ],
                        clock: self.clock,
                        frame: self.frame,
                        view: &kept.view,
                        encoder: &mut encoder,
                    },
                );
            }
            if let (Some(timing), Some(slot)) = (&self.timing, timing_slot) {
                timing.stamp(slot, 1, &mut encoder);
            }
            kept.restore(&self.gpu, &texture.texture, &view, &mut encoder);
            let publish = wallpaper_due
                && self.backdrop.encode(
                    &self.gpu,
                    Draw {
                        screen: index,
                        frame: self.frame,
                        size: screen.size(),
                        kept: &kept,
                        held: self.held,
                        animated,
                    },
                    &mut encoder,
                );
            // After the backdrop's copy, so the bars' blurred wallpaper has no plates in it.
            if let (Some(timing), Some(slot)) = (&self.timing, timing_slot) {
                timing.stamp(slot, 2, &mut encoder);
            }
            let glass_drawn =
                self.glass
                    .encode(&self.gpu, index, &kept, &texture.texture, &mut encoder);
            if let (Some(timing), Some(slot)) = (&self.timing, timing_slot) {
                timing.stamp(slot, 3, &mut encoder);
                timing.resolve(slot, &mut encoder);
            }
            self.gpu.queue.submit([encoder.finish()]);
            if let (Some(timing), Some(slot)) = (&mut self.timing, timing_slot) {
                timing.submitted(slot, glass_drawn);
            }
            self.gpu.queue.present(texture);
            if publish {
                self.backdrop.finish(&self.gpu, index);
            }
            if wallpaper_due {
                self.source.presented(index);
            }
            drew = true;
        }

        if drew && let Some(timing) = &mut self.timing {
            timing.presented(self.frame);
        }

        // First frame of a newly loaded theme: publish its palette, once.
        if drew && std::mem::take(&mut self.palette_due) {
            startup::ready("wallpaper", "first_frame_ms");
            if let Some(kept) = self.screens.iter().find_map(|s| s.kept.as_ref()) {
                palette::publish(&self.gpu, kept, &self.theme.dir);
            }
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

    /// Gives every visible swapchain screen a kept frame at the source's render size. When the
    /// source draws one frame for every screen, screens of one size share a single kept frame:
    /// one wallpaper pass and one texture instead of one per screen.
    fn assign_kept_frames(&mut self) {
        let shared = self.source.shared();
        for index in 0..self.screens.len() {
            let screen = &self.screens[index];
            let size = self.source.render_size(screen.size());
            if screen.paused
                || self.source.native(index)
                || screen.kept.as_ref().is_some_and(|k| k.size() == size)
            {
                continue;
            }
            let sibling = self
                .screens
                .iter()
                .filter_map(|s| s.kept.as_ref())
                .find(|k| shared && k.size() == size)
                .cloned();
            self.screens[index].kept =
                Some(sibling.unwrap_or_else(|| Rc::new(KeptFrame::new(&self.gpu, size))));
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
        let id = theme
            .dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if self.renderer_config.resolve(id, theme.theme.engine).gpu != self.gpu.preference {
            self.theme = theme;
            self.refresh_renderer();
            return Ok(());
        }
        let started = Instant::now();
        let hwnds: Vec<HWND> = self.screens.iter().map(|s| s.window.0).collect();
        let source = sources::load(&self.gpu, &theme, &hwnds)?;
        log::info!("{} loaded in {:.1?}", theme.theme.name, started.elapsed());
        self.glass.reset();
        for screen in &mut self.screens {
            screen.kept = None;
        }
        self.source = source;
        self.theme = theme;
        self.refresh_renderer();
        self.palette_due = true;
        self.backdrop.reset(|i| self.source.native(i));
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

/// A wallpaper cap never throttles plate geometry. Timed/static sources keep their own policy.
fn cap_redraw(redraw: Redraw, fps: Option<u32>) -> Redraw {
    match (redraw, fps) {
        (Redraw::Every(interval), Some(fps)) => {
            Redraw::Every(interval.max(Duration::from_secs(1) / fps))
        }
        (other, _) => other,
    }
}
#[cfg(test)]
mod renderer_tests {
    use super::*;
    #[test]
    fn caps_only_slow_animated_wallpaper_frames() {
        let frame = Duration::from_secs(1) / 60;
        assert_eq!(
            cap_redraw(Redraw::Every(frame), Some(30)),
            Redraw::Every(Duration::from_secs(1) / 30)
        );
        assert_eq!(
            cap_redraw(Redraw::Every(frame), Some(240)),
            Redraw::Every(frame)
        );
        assert_eq!(cap_redraw(Redraw::OnChange, Some(30)), Redraw::OnChange);
        assert_eq!(
            cap_redraw(Redraw::At(Duration::from_secs(5)), Some(30)),
            Redraw::At(Duration::from_secs(5))
        );
    }
}
