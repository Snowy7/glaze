//! History stills occupy the bottom of the desktop band. They never activate or play media.
use mypaper_desktop::Monitor;
use mypaper_theme::{Fit, LoadedTheme, Wallpaper};
use mypaper_ui::{DisplayList, Image, Kind, Op, Rect, Rgba, Role, Surface, Ui};
use std::{path::PathBuf, sync::Arc};

pub fn still_path(theme: &LoadedTheme) -> Option<PathBuf> {
    let actual = match &theme.theme.wallpaper {
        Wallpaper::Image(image) => Some(theme.asset(&image.source)),
        Wallpaper::Slideshow(slideshow) => {
            let mut files: Vec<_> = std::fs::read_dir(theme.asset(&slideshow.folder))
                .ok()?
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                        matches!(
                            e.to_ascii_lowercase().as_str(),
                            "png" | "jpg" | "jpeg" | "webp" | "bmp"
                        )
                    })
                })
                .collect();
            files.sort();
            files.into_iter().next()
        }
        _ => None,
    };
    actual.filter(|p| p.is_file()).or_else(|| {
        ["preview.png", "preview.jpg", "preview.jpeg", "preview.webp"]
            .into_iter()
            .map(|name| theme.dir.join(name))
            .find(|p| p.is_file())
    })
}

fn fit(theme: &LoadedTheme) -> Fit {
    match &theme.theme.wallpaper {
        Wallpaper::Image(i) => i.fit,
        Wallpaper::Slideshow(i) => i.fit,
        Wallpaper::Video(i) => i.fit,
        _ => Fit::Cover,
    }
}

fn image_rect(size: (f32, f32), screen: Rect, fit: Fit) -> Rect {
    let scale = match fit {
        Fit::Cover => (screen.width() / size.0).max(screen.height() / size.1),
        Fit::Contain => (screen.width() / size.0).min(screen.height() / size.1),
        Fit::Center => 1.0,
        Fit::Stretch => return screen,
    };
    let (w, h) = (size.0 * scale, size.1 * scale);
    Rect::new(
        screen.left + (screen.width() - w) / 2.0,
        screen.top + (screen.height() - h) / 2.0,
        w,
        h,
    )
}

#[derive(Default)]
pub struct Still {
    stamp: Option<(u64, Vec<(String, mypaper_desktop::Rect)>)>,
    surfaces: Vec<Surface>,
}
impl Still {
    pub fn clear(&mut self) {
        self.surfaces.clear();
        self.stamp = None;
    }

    /// Called after widget reconciliation so new still HWNDs are below all previewed widgets.
    pub fn update(
        &mut self,
        ui: &mut Ui,
        sequence: Option<u64>,
        theme: Option<&LoadedTheme>,
        monitors: &[Monitor],
        anchor: Option<windows::Win32::Foundation::HWND>,
    ) -> anyhow::Result<()> {
        let (Some(sequence), Some(theme), Some(parent)) = (sequence, theme, anchor) else {
            self.clear();
            return Ok(());
        };
        let stamp = (
            sequence,
            monitors
                .iter()
                .map(|m| (m.device.clone(), m.rect))
                .collect(),
        );
        if self.stamp.as_ref() == Some(&stamp) {
            return Ok(());
        }
        self.clear();
        self.stamp = Some(stamp);
        let Some(path) = still_path(theme) else {
            return Ok(());
        };
        let image = Arc::new(Image::load(ui, &path)?);
        for monitor in monitors {
            let r = monitor.rect;
            let screen = Rect::new(0., 0., r.width as f32, r.height as f32);
            let mut surface = Surface::new(
                Kind::DesktopChild { parent },
                "Studio history wallpaper still",
                Role::Pane,
            )?;
            surface.set_no_activate(true);
            surface.set_click_through(true);
            surface.present(
                ui,
                (r.x, r.y),
                (r.width as u32, r.height as u32),
                DisplayList {
                    ops: vec![
                        Op::Fill {
                            rect: screen,
                            radius: 0.,
                            color: Rgba::rgb8([18, 18, 18], 1.),
                        },
                        Op::Clipped {
                            rect: screen,
                            ops: vec![Op::Image {
                                rect: image_rect(
                                    (image.width as f32, image.height as f32),
                                    screen,
                                    fit(theme),
                                ),
                                image: image.clone(),
                                opacity: 1.,
                                source: None,
                            }]
                            .into(),
                        },
                    ],
                    ..Default::default()
                },
            )?;
            surface.show();
            self.surfaces.push(surface);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn past_still_obeys_its_own_fit_on_each_display() {
        let screen = Rect::new(0., 0., 1920., 1080.);
        let cover = image_rect((100., 100.), screen, Fit::Cover);
        assert_eq!(cover.width(), 1920.);
        assert_eq!(cover.height(), 1920.);
        let contain = image_rect((100., 100.), screen, Fit::Contain);
        assert_eq!(contain.width(), 1080.);
        assert_eq!(contain.left, 420.);
        assert_eq!(image_rect((100., 100.), screen, Fit::Stretch), screen);
        assert_eq!(image_rect((100., 100.), screen, Fit::Center).width(), 100.);
    }
}
