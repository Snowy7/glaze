//! Wallpaper sources: one per theme `kind`. A source draws into a screen's
//! swapchain image and says when it next needs to draw. The engine owns
//! pacing, pausing and clocks; sources own their GPU resources.
//!
//! Lightness contract: a source must do *no* work for frames it does not
//! need. Static content returns [`Redraw::OnChange`] and is drawn exactly
//! once per screen (plus after resizes / resumes / reloads).

mod app;
pub mod blit;
mod image;
mod scene;
mod shader;
mod slideshow;
mod video;
mod web;
mod wic;

use std::time::Duration;

use anyhow::Result;
use mypaper_theme::{LoadedTheme, Wallpaper};
use windows::Win32::Foundation::HWND;

use crate::gpu::Gpu;

/// Everything a source needs to draw one screen.
pub struct Frame<'a> {
    /// Stable index of the screen (sources keep per-screen state by it).
    pub screen: usize,
    /// Swapchain size in pixels.
    pub size: (u32, u32),
    /// Cursor in pixels, relative to this screen, top-left origin.
    pub mouse: [f32; 2],
    /// Engine clock: advances only while some screen is visible.
    pub clock: Duration,
    pub frame: u32,
    pub view: &'a wgpu::TextureView,
    pub encoder: &'a mut wgpu::CommandEncoder,
}

/// When a source next needs to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Redraw {
    /// Animated: draw at this interval (the fps cap).
    Every(Duration),
    /// Draw once the engine clock reaches this time (e.g. next slide).
    At(Duration),
    /// Static: only when the engine marks a screen dirty.
    OnChange,
}

pub trait Source {
    /// Size of the kept wallpaper texture; scaled shaders render directly into this target.
    fn render_size(&self, size: (u32, u32)) -> (u32, u32) {
        size
    }
    /// Advances time-based state once per engine tick, before any drawing
    /// (decode the next video frame, advance a slideshow, ...).
    fn update(&mut self, _gpu: &Gpu, _clock: Duration) {}
    /// Records the commands that draw one screen.
    fn render(&mut self, gpu: &Gpu, frame: Frame<'_>);
    fn redraw(&self, clock: Duration) -> Redraw;

    /// True while `screen` is drawn by a child window of the source's own (a
    /// WebView, an external app) instead of the swapchain. The engine then
    /// never acquires or presents a frame for it; a dirty screen calls
    /// [`layout`](Self::layout) instead of [`render`](Self::render). A native
    /// source may hand a screen back to the swapchain (e.g. to show a still
    /// of itself while paused) by returning false.
    fn native(&self, _screen: usize) -> bool {
        false
    }
    /// True once (then false) when `screen` has a frame that must be
    /// presented even while the engine is held, e.g. a still captured just
    /// before a WebView is hidden.
    fn take_dirty(&mut self, _screen: usize) -> bool {
        false
    }
    /// `screen`'s frame was just presented.
    fn presented(&mut self, _screen: usize) {}
    /// Native sources: a screen needs (re)laying out because it is new,
    /// resized, resumed or reloaded. `size` is its client size in pixels.
    fn layout(&mut self, _screen: usize, _hwnd: HWND, _size: (u32, u32)) {}
    /// A screen's [`Visibility`] changed: covered, minimized, or the whole
    /// engine held or released. Sources that keep running something of their
    /// own (a browser, a process) suspend it unless it is `Live`. Every screen
    /// is `Live` when a source is created.
    fn set_visibility(&mut self, _screen: usize, _hwnd: HWND, _visibility: Visibility) {}
}

/// How much of a screen anyone can see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// On screen and running.
    Live,
    /// Paused but still seen (held while uncovered, e.g. the second monitor
    /// while a game is fullscreen): show a still of the wallpaper.
    Still,
    /// Nobody can see it (covered or minimized): show nothing, cost nothing.
    Hidden,
}

impl Visibility {
    /// What a screen is: covered/minimized wins over held.
    pub fn of(held: bool, covered: bool) -> Self {
        match (covered, held) {
            (true, _) => Self::Hidden,
            (false, true) => Self::Still,
            (false, false) => Self::Live,
        }
    }
}

/// Builds the source for a theme. `screens` are the engine's screen windows,
/// in screen order, for sources that create child windows of their own.
pub fn load(gpu: &Gpu, theme: &LoadedTheme, screens: &[HWND]) -> Result<Box<dyn Source>> {
    Ok(match &theme.theme.wallpaper {
        Wallpaper::Shader(w) => Box::new(shader::ShaderSource::new(gpu, theme, w)?),
        Wallpaper::Image(w) => Box::new(image::ImageSource::new(gpu, theme, w)?),
        Wallpaper::Slideshow(w) => Box::new(slideshow::SlideshowSource::new(gpu, theme, w)?),
        Wallpaper::Video(w) => Box::new(video::VideoSource::new(gpu, theme, w)?),
        Wallpaper::Scene(w) => Box::new(scene::SceneSource::new(gpu, theme, w)?),
        Wallpaper::Web(w) => Box::new(web::WebSource::new(theme, w, screens)?),
        Wallpaper::App(w) => Box::new(app::AppSource::new(theme, w, screens)?),
    })
}

/// Shown when a theme fails to load at startup: a dim red screen, so a
/// broken theme is obvious rather than silently black. Fix the theme and
/// it hot-reloads over this.
pub struct Broken;

impl Source for Broken {
    fn render(&mut self, _gpu: &Gpu, frame: Frame<'_>) {
        clear(
            frame.encoder,
            frame.view,
            wgpu::Color {
                r: 0.18,
                g: 0.0,
                b: 0.02,
                a: 1.0,
            },
        );
    }
    fn redraw(&self, _clock: Duration) -> Redraw {
        Redraw::OnChange
    }
}

/// A render pass that only clears `view`.
pub fn clear(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, color: wgpu::Color) {
    begin(encoder, view, wgpu::LoadOp::Clear(color));
}

/// Begins a single-color-attachment pass on `view`.
pub fn begin<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("wallpaper"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::Visibility;

    #[test]
    fn covered_beats_held_and_held_but_seen_is_a_still() {
        assert_eq!(Visibility::of(false, false), Visibility::Live);
        assert_eq!(Visibility::of(true, false), Visibility::Still);
        assert_eq!(Visibility::of(false, true), Visibility::Hidden);
        assert_eq!(Visibility::of(true, true), Visibility::Hidden);
    }
}
