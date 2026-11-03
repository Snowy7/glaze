//! `slideshow` wallpapers. (Stub: being implemented.)

use std::time::Duration;

use anyhow::{Result, bail};
use mypaper_theme::{LoadedTheme, SlideshowWallpaper};

use super::{Frame, Redraw, Source};
use crate::gpu::Gpu;

pub struct SlideshowSource;

impl SlideshowSource {
    pub fn new(_gpu: &Gpu, _theme: &LoadedTheme, _config: &SlideshowWallpaper) -> Result<Self> {
        bail!("slideshow wallpapers are not implemented yet")
    }
}

impl Source for SlideshowSource {
    fn render(&mut self, _gpu: &Gpu, _frame: Frame<'_>) {}

    fn redraw(&self, _clock: Duration) -> Redraw {
        Redraw::OnChange
    }
}
