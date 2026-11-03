//! `image` wallpapers. (Stub: being implemented.)

use std::time::Duration;

use anyhow::{Result, bail};
use mypaper_theme::{ImageWallpaper, LoadedTheme};

use super::{Frame, Redraw, Source};
use crate::gpu::Gpu;

pub struct ImageSource;

impl ImageSource {
    pub fn new(_gpu: &Gpu, _theme: &LoadedTheme, _config: &ImageWallpaper) -> Result<Self> {
        bail!("image wallpapers are not implemented yet")
    }
}

impl Source for ImageSource {
    fn render(&mut self, _gpu: &Gpu, _frame: Frame<'_>) {}

    fn redraw(&self, _clock: Duration) -> Redraw {
        Redraw::OnChange
    }
}
