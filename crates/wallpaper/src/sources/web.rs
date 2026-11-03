//! `web` wallpapers. (Stub: being implemented.)

use std::time::Duration;

use anyhow::{Result, bail};
use mypaper_theme::{LoadedTheme, WebWallpaper};

use super::{Frame, Redraw, Source};
use crate::gpu::Gpu;

pub struct WebSource;

impl WebSource {
    pub fn new(_gpu: &Gpu, _theme: &LoadedTheme, _config: &WebWallpaper) -> Result<Self> {
        bail!("web wallpapers are not implemented yet")
    }
}

impl Source for WebSource {
    fn render(&mut self, _gpu: &Gpu, _frame: Frame<'_>) {}

    fn redraw(&self, _clock: Duration) -> Redraw {
        Redraw::OnChange
    }
}
