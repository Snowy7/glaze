//! Shared image decoding through the Windows Imaging Component (WIC), so
//! jpg, png, bmp, gif, tiff, webp and heic all work without extra crates.
//! Images are decoded to straight-alpha BGRA and downscaled on the way out,
//! so we never keep more pixels than the biggest screen can show.

use std::path::Path;

use anyhow::{Context, Result, bail};
use mypaper_theme::Fit;
use windows::Win32::Foundation::{GENERIC_READ, RPC_E_CHANGED_MODE};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICImagingFactory, IWICPalette,
    WICBitmapDitherTypeNone, WICBitmapInterpolationModeHighQualityCubic,
    WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::core::HSTRING;

use super::blit::BlitQuad;
use crate::gpu::Gpu;

/// Used when no monitor can be enumerated.
const FALLBACK_SCREEN: (u32, u32) = (3840, 2160);

/// A decoded image: tightly packed straight-alpha BGRA.
pub struct Decoded {
    pub size: (u32, u32),
    pub bgra: Vec<u8>,
}

/// How many pixels a decoded image may keep: enough for its [`Fit`] mode on
/// the largest screen, and never more than the GPU can hold in one texture.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub fit: Fit,
    pub screen: (u32, u32),
    pub max_dim: u32,
}

impl Budget {
    /// The budget for `fit` on this machine's monitors and GPU.
    pub fn new(gpu: &Gpu, fit: Fit) -> Self {
        Self {
            fit,
            screen: largest_screen(),
            max_dim: gpu.device.limits().max_texture_dimension_2d,
        }
    }

    /// The size an `image`-sized picture is decoded at. Never upscales.
    pub fn size_for(&self, image: (u32, u32)) -> (u32, u32) {
        let (iw, ih) = (u64::from(image.0.max(1)), u64::from(image.1.max(1)));
        let (sw, sh) = (
            u64::from(self.screen.0.max(1)),
            u64::from(self.screen.1.max(1)),
        );
        let size = match self.fit {
            Fit::Center => (iw, ih),
            Fit::Stretch => (iw.min(sw), ih.min(sh)),
            // Scaling is only free when the image is bigger than the screen on
            // both axes; otherwise it already can't cover (or doesn't need to shrink).
            Fit::Cover if iw > sw && ih > sh => {
                if sw * ih >= sh * iw {
                    scale_to_width((iw, ih), sw)
                } else {
                    scale_to_height((iw, ih), sh)
                }
            }
            Fit::Contain if iw > sw || ih > sh => {
                if sw * ih <= sh * iw {
                    scale_to_width((iw, ih), sw)
                } else {
                    scale_to_height((iw, ih), sh)
                }
            }
            Fit::Cover | Fit::Contain => (iw, ih),
        };
        let max = u64::from(self.max_dim.max(1));
        let size = if size.0 > max {
            scale_to_width(size, max)
        } else {
            size
        };
        let size = if size.1 > max {
            scale_to_height(size, max)
        } else {
            size
        };
        (size.0 as u32, size.1 as u32)
    }
}

/// Resizes to `width`, keeping the aspect ratio (rounding the height up).
fn scale_to_width((w, h): (u64, u64), width: u64) -> (u64, u64) {
    (width, (h * width).div_ceil(w).max(1))
}

/// Resizes to `height`, keeping the aspect ratio (rounding the width up).
fn scale_to_height((w, h): (u64, u64), height: u64) -> (u64, u64) {
    ((w * height).div_ceil(h).max(1), height)
}

/// The bounding box of all monitors' sizes: an image that covers it covers
/// every monitor.
fn largest_screen() -> (u32, u32) {
    let (w, h) = mypaper_desktop::monitors()
        .iter()
        .fold((0, 0), |(w, h), m| {
            (
                w.max(u32::try_from(m.width).unwrap_or(0)),
                h.max(u32::try_from(m.height).unwrap_or(0)),
            )
        });
    if w == 0 || h == 0 {
        FALLBACK_SCREEN
    } else {
        (w, h)
    }
}

/// Keeps COM initialized on the creating thread. Tolerates a thread that
/// already chose another apartment model (`RPC_E_CHANGED_MODE`) or is
/// already initialized (`S_FALSE`).
struct ComGuard {
    /// Whether our `CoInitializeEx` succeeded and so needs a matching uninit.
    owned: bool,
}

impl ComGuard {
    fn new() -> Result<Self> {
        // SAFETY: plain per-thread COM init; balanced by `Drop` on success.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr == RPC_E_CHANGED_MODE {
            return Ok(Self { owned: false });
        }
        hr.ok().context("initializing COM")?;
        Ok(Self { owned: true })
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: balances the successful `CoInitializeEx` in `new`, on the same thread
            // (`Decoder` holds COM objects and is therefore not `Send`).
            unsafe { CoUninitialize() };
        }
    }
}

/// A WIC factory bound to the thread that created it. Create one per
/// thread that decodes.
pub struct Decoder {
    // Field order matters: COM objects must be released before COM is uninitialized.
    factory: IWICImagingFactory,
    _com: ComGuard,
}

impl Decoder {
    /// Initializes COM on this thread and creates the WIC factory.
    pub fn new() -> Result<Self> {
        let com = ComGuard::new()?;
        // SAFETY: COM is initialized on this thread; the CLSID names the WIC factory.
        let factory: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
                .context("creating the WIC imaging factory")?;
        Ok(Self { factory, _com: com })
    }

    /// Decodes the first frame of `path` to BGRA, downscaled to `budget`.
    /// Format conversion and scaling stream through WIC's pipeline, so only
    /// the final (small) buffer is ever allocated.
    pub fn decode(&self, path: &Path, budget: Budget) -> Result<Decoded> {
        let name = HSTRING::from(path.as_os_str());
        let (mut width, mut height) = (0u32, 0u32);
        // SAFETY: every call is on COM objects owned by this thread's factory;
        // `CopyPixels` writes at most `stride * height` bytes into `bgra`, which has exactly that length.
        let (size, bgra) = unsafe {
            let decoder = self.factory.CreateDecoderFromFilename(
                &name,
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )?;
            let frame = decoder.GetFrame(0)?;
            frame.GetSize(&mut width, &mut height)?;
            if width == 0 || height == 0 {
                bail!("image has no pixels");
            }

            let converter = self.factory.CreateFormatConverter()?;
            converter.Initialize(
                &frame,
                &GUID_WICPixelFormat32bppBGRA,
                WICBitmapDitherTypeNone,
                None::<&IWICPalette>,
                0.0,
                WICBitmapPaletteTypeCustom,
            )?;

            let size = budget.size_for((width, height));
            let stride = size.0 * 4;
            let mut bgra = vec![0u8; stride as usize * size.1 as usize];
            if size == (width, height) {
                converter.CopyPixels(std::ptr::null(), stride, &mut bgra)?;
            } else {
                let scaler = self.factory.CreateBitmapScaler()?;
                scaler.Initialize(
                    &converter,
                    size.0,
                    size.1,
                    WICBitmapInterpolationModeHighQualityCubic,
                )?;
                scaler.CopyPixels(std::ptr::null(), stride, &mut bgra)?;
            }
            (size, bgra)
        };
        Ok(Decoded { size, bgra })
    }
}

/// A decoded image living on the GPU, drawable with any fit and opacity.
pub struct Picture {
    // The quad's bind group already keeps the texture alive; held for clarity.
    _texture: wgpu::Texture,
    quad: BlitQuad,
    size: (u32, u32),
}

impl Picture {
    /// Uploads `image` to a new `Bgra8Unorm` texture. Fails (instead of
    /// panicking) if the GPU rejects it.
    pub fn upload(gpu: &Gpu, label: &str, image: &Decoded) -> Result<Self> {
        let texture = gpu.checked(|| {
            gpu.texture(
                label,
                image.size,
                wgpu::TextureFormat::Bgra8Unorm,
                wgpu::TextureUsages::empty(),
                Some(&image.bgra),
            )
        })?;
        let view = texture.create_view(&Default::default());
        let quad = gpu.blit.quad(&gpu.device, &view);
        Ok(Self {
            _texture: texture,
            quad,
            size: image.size,
        })
    }

    /// Draws over the whole target in `pass`, mapped by `fit` and blended at `opacity`.
    pub fn draw(
        &self,
        gpu: &Gpu,
        pass: &mut wgpu::RenderPass<'_>,
        target: (u32, u32),
        fit: Fit,
        opacity: f32,
    ) {
        self.quad.set(&gpu.queue, self.size, target, fit, opacity);
        self.quad.draw(&gpu.blit, pass);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget(fit: Fit) -> Budget {
        Budget {
            fit,
            screen: (1920, 1080),
            max_dim: 16384,
        }
    }

    #[test]
    fn cover_shrinks_to_just_cover_the_screen() {
        // 4:3 on 16:9: width binds, height overshoots (and gets cropped later).
        assert_eq!(budget(Fit::Cover).size_for((4000, 3000)), (1920, 1440));
        // Ultra-wide: height binds.
        assert_eq!(budget(Fit::Cover).size_for((8000, 2000)), (4320, 1080));
    }

    #[test]
    fn cover_never_shrinks_below_the_screen_on_either_axis() {
        // Wider than the screen but too short to shrink: must stay as is.
        assert_eq!(budget(Fit::Cover).size_for((4000, 900)), (4000, 900));
        assert_eq!(budget(Fit::Cover).size_for((1600, 900)), (1600, 900));
    }

    #[test]
    fn contain_fits_inside_the_screen() {
        assert_eq!(budget(Fit::Contain).size_for((4000, 3000)), (1440, 1080));
        assert_eq!(budget(Fit::Contain).size_for((1000, 500)), (1000, 500));
    }

    #[test]
    fn stretch_shrinks_each_axis_and_center_never_scales() {
        assert_eq!(budget(Fit::Stretch).size_for((4000, 500)), (1920, 500));
        assert_eq!(budget(Fit::Center).size_for((4000, 3000)), (4000, 3000));
    }

    #[test]
    fn nothing_exceeds_the_texture_limit() {
        let tight = Budget {
            max_dim: 2048,
            ..budget(Fit::Center)
        };
        assert_eq!(tight.size_for((8192, 4096)), (2048, 1024));
        assert_eq!(tight.size_for((1000, 6000)), (342, 2048));
    }
}
