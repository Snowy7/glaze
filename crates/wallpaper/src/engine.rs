//! The frame loop policy: when to render, when to sleep, when to reload.
//! Everything here is hot-patchable with `dx serve --hotpatch`; keep struct
//! layouts stable while patching (change function bodies, not fields).

use std::time::{Duration, Instant};

use anyhow::Result;
use mypaper_desktop::{EXIT_RESTART, Rect, WindowEvents, covered_monitors, monitors};
use mypaper_theme::{LoadedTheme, Wallpaper};
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::gpu::{Gpu, Screen, Uniforms};
use crate::window::Window;

/// Coalesces the burst of file events an editor save produces.
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(40);
/// Coalesces window-move storms before re-checking what is covered.
const COVER_DEBOUNCE: Duration = Duration::from_millis(100);

pub enum Flow {
    /// Sleep until this deadline (or the next message, if `None`).
    Wait(Option<Instant>),
    Exit(u8),
}

pub struct Engine {
    theme: LoadedTheme,
    gpu: Gpu,
    pipeline: Option<wgpu::RenderPipeline>,
    /// The shader source last compiled (successfully or not).
    source: String,
    screens: Vec<Screen>,
    /// Monitor rects for desktop mode; `None` in dev (windowed) mode.
    monitors: Option<Vec<Rect>>,
    interval: Duration,
    next_frame: Instant,
    last_frame: Option<Instant>,
    time: Duration,
    frame: u32,
    reload_at: Option<Instant>,
    recheck_at: Option<Instant>,
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
        let mut engine = Self {
            theme,
            gpu,
            pipeline: None,
            source: String::new(),
            screens,
            monitors,
            interval: Duration::from_secs(1) / 60,
            next_frame: Instant::now(),
            last_frame: None,
            time: Duration::ZERO,
            frame: 0,
            reload_at: None,
            recheck_at: None,
        };
        engine.apply_theme();
        Ok(engine)
    }

    pub fn schedule_reload(&mut self) {
        self.reload_at = Some(Instant::now() + RELOAD_DEBOUNCE);
    }

    /// One turn of the loop: react to changes, render if a frame is due,
    /// and say how long the thread may sleep.
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
                        log::info!(
                            "{} {}",
                            if covered { "pausing" } else { "resuming" },
                            label(screen)
                        );
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
                screen.paused = screen.window.minimized();
                screen.sync_size(&self.gpu);
            }
        }

        let active = self.screens.iter().any(|s| !s.paused);
        if !active {
            self.last_frame = None; // so time resumes where it stopped
        } else if now >= self.next_frame {
            self.render(now);
            self.next_frame = (self.next_frame + self.interval).max(now);
        }

        let frame_deadline = active.then_some(self.next_frame);
        Flow::Wait(
            [frame_deadline, self.reload_at, self.recheck_at]
                .into_iter()
                .flatten()
                .min(),
        )
    }

    fn render(&mut self, now: Instant) {
        // Cap the step so resuming after a pause continues smoothly.
        let dt = self
            .last_frame
            .map_or(Duration::ZERO, |last| (now - last).min(self.interval * 4));
        self.last_frame = Some(now);
        self.time += dt;
        self.frame = self.frame.wrapping_add(1);

        let mut cursor = POINT::default();
        // SAFETY: `cursor` outlives the call.
        let _ = unsafe { GetCursorPos(&mut cursor) };

        for screen in self.screens.iter_mut().filter(|s| !s.paused) {
            let mut mouse = cursor;
            // SAFETY: valid window; `mouse` outlives the call.
            let _ = unsafe { ScreenToClient(screen.window.0, &mut mouse) };
            let (w, h) = screen.size();
            let uniforms = Uniforms {
                resolution: [w as f32, h as f32],
                mouse: [mouse.x as f32, mouse.y as f32],
                time: self.time.as_secs_f32(),
                frame: self.frame,
                _pad: [0.0; 2],
            };
            screen.render(&self.gpu, self.pipeline.as_ref(), &uniforms);
        }
    }

    fn reload(&mut self) {
        match LoadedTheme::load(&self.theme.dir) {
            Ok(theme) => self.theme = theme,
            Err(e) => {
                log::error!("{e:#}");
                return;
            }
        }
        self.apply_theme();
    }

    fn apply_theme(&mut self) {
        let Wallpaper::Shader(shader) = &self.theme.theme.wallpaper;
        self.interval = Duration::from_secs(1) / shader.fps;
        let path = self.theme.asset(&shader.source);
        let source = match std::fs::read_to_string(&path) {
            Ok(source) => source,
            Err(e) => {
                log::error!("{}: {e}", path.display());
                return;
            }
        };
        // Editors fire several events per save; compile each version once.
        if source == self.source {
            return;
        }
        let started = Instant::now();
        let compiled = self.gpu.compile(&source);
        self.source = source;
        match compiled {
            Ok(pipeline) => {
                self.pipeline = Some(pipeline);
                log::info!("{} ready in {:.1?}", path.display(), started.elapsed());
            }
            Err(e) => log::error!("{}: {e:#}\n(keeping the previous shader)", path.display()),
        }
        self.next_frame = Instant::now();
    }
}

fn label(screen: &Screen) -> String {
    let (w, h) = screen.size();
    format!("{w}x{h} screen")
}
