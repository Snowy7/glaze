//! `video` wallpapers. (Stub: being implemented.)

use std::time::Duration;

use anyhow::{Result, bail};
use mypaper_theme::{LoadedTheme, VideoWallpaper};

use super::{Frame, Redraw, Source};
use crate::gpu::Gpu;

pub struct VideoSource;

impl VideoSource {
    pub fn new(_gpu: &Gpu, _theme: &LoadedTheme, _config: &VideoWallpaper) -> Result<Self> {
        bail!("video wallpapers are not implemented yet")
    }
}

impl Source for VideoSource {
    fn render(&mut self, _gpu: &Gpu, _frame: Frame<'_>) {}

    fn redraw(&self, _clock: Duration) -> Redraw {
        Redraw::OnChange
    }
}
