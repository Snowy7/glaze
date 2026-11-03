//! `app` wallpapers. (Stub: being implemented.)

use std::time::Duration;

use anyhow::{Result, bail};
use mypaper_theme::{AppWallpaper, LoadedTheme};

use super::{Frame, Redraw, Source};
use crate::gpu::Gpu;

pub struct AppSource;

impl AppSource {
    pub fn new(_gpu: &Gpu, _theme: &LoadedTheme, _config: &AppWallpaper) -> Result<Self> {
        bail!("app wallpapers are not implemented yet")
    }
}

impl Source for AppSource {
    fn render(&mut self, _gpu: &Gpu, _frame: Frame<'_>) {}

    fn redraw(&self, _clock: Duration) -> Redraw {
        Redraw::OnChange
    }
}
