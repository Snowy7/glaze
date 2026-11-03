//! `scene` wallpapers. (Stub: being implemented.)

use std::time::Duration;

use anyhow::{Result, bail};
use mypaper_theme::{LoadedTheme, SceneWallpaper};

use super::{Frame, Redraw, Source};
use crate::gpu::Gpu;

pub struct SceneSource;

impl SceneSource {
    pub fn new(_gpu: &Gpu, _theme: &LoadedTheme, _config: &SceneWallpaper) -> Result<Self> {
        bail!("scene wallpapers are not implemented yet")
    }
}

impl Source for SceneSource {
    fn render(&mut self, _gpu: &Gpu, _frame: Frame<'_>) {}

    fn redraw(&self, _clock: Duration) -> Redraw {
        Redraw::OnChange
    }
}
