//! `image` wallpapers: decoded once, drawn once per screen, then free.

use std::time::Duration;

use anyhow::{Context, Result};
use mypaper_theme::{Fit, ImageWallpaper, LoadedTheme};

use super::wic::{Budget, Decoder, Picture};
use super::{Frame, Redraw, Source, begin};
use crate::gpu::Gpu;

/// One still image, resident on the GPU at no more than the largest screen's resolution.
pub struct ImageSource {
    picture: Picture,
    fit: Fit,
}

impl ImageSource {
    /// Decodes and uploads the image. Any failure is returned so the engine
    /// shows its "broken" screen until the theme is fixed.
    pub fn new(gpu: &Gpu, theme: &LoadedTheme, config: &ImageWallpaper) -> Result<Self> {
        let path = theme.asset(&config.source);
        let decoded = Decoder::new()?
            .decode(&path, Budget::new(gpu, config.fit))
            .with_context(|| format!("decoding {}", path.display()))?;
        let picture = Picture::upload(gpu, "image", &decoded)
            .with_context(|| format!("uploading {}", path.display()))?;
        Ok(Self {
            picture,
            fit: config.fit,
        })
    }
}

impl Source for ImageSource {
    fn render(&mut self, gpu: &Gpu, frame: Frame<'_>) {
        let mut pass = begin(
            frame.encoder,
            frame.view,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        self.picture.draw(gpu, &mut pass, frame.size, self.fit, 1.0);
    }

    fn redraw(&self, _clock: Duration) -> Redraw {
        Redraw::OnChange
    }

    fn shared(&self) -> bool {
        true
    }
}
