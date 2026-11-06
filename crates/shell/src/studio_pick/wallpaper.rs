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
                .into_iter()
                .flatten()
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

/// Decode at a display-sized bound, retaining native dimensions for pixel-for-pixel Center.
fn load_still(path: &std::path::Path, max: (u32, u32)) -> anyhow::Result<(Image, (f32, f32))> {
    use windows::Win32::{
        Foundation::GENERIC_READ,
        Graphics::Imaging::*,
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
    };
    // SAFETY: Ui initialized COM on this thread; local WIC objects copy into a bounded BGRA buffer.
    unsafe {
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let decoder = wic.CreateDecoderFromFilename(
            &windows::core::HSTRING::from(path.as_os_str()),
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnLoad,
        )?;
        let frame = decoder.GetFrame(0)?;
        let (mut w, mut h) = (0, 0);
        frame.GetSize(&mut w, &mut h)?;
        anyhow::ensure!(
            w > 0 && h > 0 && w <= 65536 && h <= 65536,
            "wallpaper still dimensions"
        );
        let native = (w as f32, h as f32);
        let scale = (max.0.min(4096) as f64 / w as f64)
            .min(max.1.min(4096) as f64 / h as f64)
            .min(1.);
        let width = (w as f64 * scale).round().max(1.) as u32;
        let height = (h as f64 * scale).round().max(1.) as u32;
        let scaler = wic.CreateBitmapScaler()?;
        scaler.Initialize(&frame, width, height, WICBitmapInterpolationModeFant)?;
        let converter = wic.CreateFormatConverter()?;
        converter.Initialize(
            &scaler,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.,
            WICBitmapPaletteTypeCustom,
        )?;
        let mut pixels = vec![0; width as usize * height as usize * 4];
        converter.CopyPixels(std::ptr::null(), width * 4, &mut pixels)?;
        Ok((
            Image {
                width,
                height,
                pixels,
            },
            native,
        ))
    }
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
    stamp: Option<Stamp>,
    surfaces: Vec<Surface>,
}

#[derive(PartialEq)]
struct Stamp {
    sequence: u64,
    monitors: Vec<(String, mypaper_desktop::Rect)>,
    widgets: Vec<(String, mypaper_ui::SurfaceId)>,
    parent: isize,
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
        widgets: Vec<(String, mypaper_ui::SurfaceId)>,
    ) -> anyhow::Result<()> {
        let (Some(sequence), Some(theme), Some(parent)) = (sequence, theme, anchor) else {
            self.clear();
            return Ok(());
        };
        let stamp = Stamp {
            sequence,
            monitors: monitors
                .iter()
                .map(|m| (m.device.clone(), m.rect))
                .collect(),
            widgets,
            parent: parent.0 as isize,
        };
        if self.stamp.as_ref() == Some(&stamp) {
            return Ok(());
        }
        self.clear();
        self.stamp = Some(stamp);
        let Some(path) = still_path(theme) else {
            return Ok(());
        };
        let max = monitors.iter().fold((1, 1), |(w, h), m| {
            (w.max(m.rect.width as u32), h.max(m.rect.height as u32))
        });
        let (image, native) = load_still(&path, max)?;
        let image = Arc::new(image);
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
                                rect: image_rect(native, screen, fit(theme)),
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
    fn large_stills_decode_with_bounded_pixels_and_retain_native_center_size() {
        let _ui = Ui::new().unwrap();
        let path =
            std::env::temp_dir().join(format!("mypaper-history-still-{}.bmp", std::process::id()));
        let mut bmp = vec![0u8; 54 + 8192 * 2 * 3];
        bmp[..2].copy_from_slice(b"BM");
        let len = bmp.len() as u32;
        bmp[2..6].copy_from_slice(&len.to_le_bytes());
        bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&8192u32.to_le_bytes());
        bmp[22..26].copy_from_slice(&2u32.to_le_bytes());
        bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
        bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
        std::fs::write(&path, bmp).unwrap();
        let result = load_still(&path, (2560, 1440));
        std::fs::remove_file(&path).unwrap();
        let (image, native) = result.unwrap();
        assert_eq!(native, (8192., 2.));
        assert_eq!(image.width, 2560);
        assert_eq!(image.height, 1);
        assert_eq!(
            image_rect(native, Rect::new(0., 0., 2560., 1440.), Fit::Center).width(),
            8192.
        );
    }
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
