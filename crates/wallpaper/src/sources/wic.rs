//! Shared image decoding through the Windows Imaging Component (WIC), so
//! jpg, png, bmp, gif, tiff, webp and heic all work without extra crates.
//! Images are decoded to straight-alpha BGRA, downscaled and turned upright
//! (EXIF orientation) on the way out, so we never keep more pixels than the
//! biggest screen can show.

use std::path::Path;

use anyhow::{Context, Result, bail};
use mypaper_theme::Fit;
use windows::Win32::Foundation::{GENERIC_READ, RPC_E_CHANGED_MODE};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICBitmapFrameDecode, IWICBitmapSource,
    IWICImagingFactory, IWICPalette, WICBitmapDitherTypeNone,
    WICBitmapInterpolationModeHighQualityCubic, WICBitmapPaletteTypeCustom,
    WICBitmapTransformFlipHorizontal, WICBitmapTransformFlipVertical, WICBitmapTransformOptions,
    WICBitmapTransformRotate0, WICBitmapTransformRotate90, WICBitmapTransformRotate180,
    WICBitmapTransformRotate270, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::core::{HSTRING, Interface, PCWSTR, w};

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

/// The EXIF orientation tag (274): how the stored pixels must be turned to
/// show the picture upright, as phones record it instead of rotating pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    /// 1, or no tag.
    #[default]
    Upright,
    /// 2
    MirrorHorizontal,
    /// 3
    Rotate180,
    /// 4
    MirrorVertical,
    /// 5: reflected across the main diagonal.
    Transpose,
    /// 6: stored rotated counter-clockwise, so turn it 90° clockwise.
    Rotate90,
    /// 7: reflected across the anti-diagonal.
    Transverse,
    /// 8: turn it 90° counter-clockwise.
    Rotate270,
}

impl Orientation {
    /// From the tag's value; `None` outside 1..=8.
    pub fn from_exif(value: u16) -> Option<Self> {
        Some(match value {
            1 => Self::Upright,
            2 => Self::MirrorHorizontal,
            3 => Self::Rotate180,
            4 => Self::MirrorVertical,
            5 => Self::Transpose,
            6 => Self::Rotate90,
            7 => Self::Transverse,
            8 => Self::Rotate270,
            _ => return None,
        })
    }

    /// The WIC transform that makes the picture upright. WIC flips first,
    /// then rotates clockwise.
    fn transform(self) -> WICBitmapTransformOptions {
        match self {
            Self::Upright => WICBitmapTransformRotate0,
            Self::MirrorHorizontal => WICBitmapTransformFlipHorizontal,
            Self::Rotate180 => WICBitmapTransformRotate180,
            Self::MirrorVertical => WICBitmapTransformFlipVertical,
            Self::Transpose => then_flip(WICBitmapTransformRotate270),
            Self::Rotate90 => WICBitmapTransformRotate90,
            Self::Transverse => then_flip(WICBitmapTransformRotate90),
            Self::Rotate270 => WICBitmapTransformRotate270,
        }
    }

    /// Whether the upright picture has the stored width and height swapped.
    pub fn swaps_axes(self) -> bool {
        matches!(
            self,
            Self::Transpose | Self::Rotate90 | Self::Transverse | Self::Rotate270
        )
    }

    /// `size` with width and height swapped if turning upright swaps them.
    /// An involution: it converts between stored and upright sizes.
    fn turn(self, (w, h): (u32, u32)) -> (u32, u32) {
        if self.swaps_axes() { (h, w) } else { (w, h) }
    }
}

/// `rotation` combined with a horizontal flip (WIC's options are plain bit
/// flags, but the generated type has no `|`).
const fn then_flip(rotation: WICBitmapTransformOptions) -> WICBitmapTransformOptions {
    WICBitmapTransformOptions(rotation.0 | WICBitmapTransformFlipHorizontal.0)
}

/// Metadata queries for the orientation tag: the Exif IFD of JPEG, then of
/// TIFF and HEIC, then Windows' format-independent property (PNG, WebP, ...).
const ORIENTATION_QUERIES: [PCWSTR; 3] = [
    w!("/app1/ifd/{ushort=274}"),
    w!("/ifd/{ushort=274}"),
    w!("System.Photo.Orientation"),
];

/// The frame's EXIF orientation. Missing, unreadable or out-of-range
/// values count as upright.
fn read_orientation(frame: &IWICBitmapFrameDecode) -> Orientation {
    // SAFETY: COM calls on live objects; `value` is a valid out-pointer.
    // Formats without metadata (BMP, ...) fail the first call.
    unsafe {
        let Ok(reader) = frame.GetMetadataQueryReader() else {
            return Orientation::Upright;
        };
        ORIENTATION_QUERIES
            .into_iter()
            .find_map(|query| {
                let mut value = PROPVARIANT::default();
                reader.GetMetadataByName(query, &mut value).ok()?;
                u16::try_from(&value).ok()
            })
            .and_then(Orientation::from_exif)
            .unwrap_or_default()
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

    /// Decodes the first frame of `path` to BGRA, downscaled to `budget` and
    /// turned upright by its EXIF orientation (`budget` sees the upright
    /// size). Format conversion and scaling stream through WIC's pipeline.
    /// The turn comes last, on the already small image: turning first would
    /// make WIC hold the whole full-size picture in memory.
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

            let orientation = read_orientation(&frame);
            // The final (upright) size, and the same size before turning.
            let size = budget.size_for(orientation.turn((width, height)));
            let scaled = orientation.turn(size);
            let mut source: IWICBitmapSource = converter.cast()?;
            if scaled != (width, height) {
                let scaler = self.factory.CreateBitmapScaler()?;
                scaler.Initialize(
                    &source,
                    scaled.0,
                    scaled.1,
                    WICBitmapInterpolationModeHighQualityCubic,
                )?;
                source = scaler.cast()?;
            }
            if orientation != Orientation::Upright {
                let rotator = self.factory.CreateBitmapFlipRotator()?;
                rotator.Initialize(&source, orientation.transform())?;
                source = rotator.cast()?;
            }
            let stride = size.0 * 4;
            let mut bgra = vec![0u8; stride as usize * size.1 as usize];
            source.CopyPixels(std::ptr::null(), stride, &mut bgra)?;
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
    use windows::Win32::Graphics::Imaging::{
        GUID_ContainerFormatJpeg, GUID_ContainerFormatTiff, GUID_WICPixelFormat24bppBGR,
        WICBitmapEncoderNoCache,
    };
    use windows::core::GUID;

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

    #[test]
    fn exif_values_map_to_wic_transforms() {
        let transform = |value| Orientation::from_exif(value).unwrap().transform();
        assert_eq!(transform(1), WICBitmapTransformRotate0);
        assert_eq!(transform(2), WICBitmapTransformFlipHorizontal);
        assert_eq!(transform(3), WICBitmapTransformRotate180);
        assert_eq!(transform(4), WICBitmapTransformFlipVertical);
        assert_eq!(
            transform(5).0,
            WICBitmapTransformRotate270.0 | WICBitmapTransformFlipHorizontal.0
        );
        assert_eq!(transform(6), WICBitmapTransformRotate90);
        assert_eq!(
            transform(7).0,
            WICBitmapTransformRotate90.0 | WICBitmapTransformFlipHorizontal.0
        );
        assert_eq!(transform(8), WICBitmapTransformRotate270);
        assert_eq!(Orientation::from_exif(0), None);
        assert_eq!(Orientation::from_exif(9), None);
    }

    #[test]
    fn only_the_quarter_turns_swap_width_and_height() {
        let swaps = |value| Orientation::from_exif(value).unwrap().swaps_axes();
        assert!((1..=4).all(|v| !swaps(v)));
        assert!((5..=8).all(swaps));
        assert_eq!(Orientation::Rotate90.turn((6, 4)), (4, 6));
        assert_eq!(Orientation::Rotate180.turn((6, 4)), (6, 4));
    }

    /// Where the upright pixel `(x, y)` of a `w × h` stored picture with EXIF
    /// `orientation` comes from, straight from the EXIF spec's table. This is
    /// the oracle the real WIC pipeline is checked against.
    fn stored_position(orientation: u16, (x, y): (u32, u32), (w, h): (u32, u32)) -> (u32, u32) {
        match orientation {
            1 => (x, y),
            2 => (w - 1 - x, y),
            3 => (w - 1 - x, h - 1 - y),
            4 => (x, h - 1 - y),
            5 => (y, x),
            6 => (y, h - 1 - x),
            7 => (w - 1 - y, h - 1 - x),
            8 => (w - 1 - y, x),
            _ => unreachable!(),
        }
    }

    /// The colour of `cell` in the test pattern: all different.
    fn cell_color((cx, cy): (u32, u32)) -> [u8; 3] {
        [(cx * 50 + 20) as u8, (cy * 50 + 20) as u8, 128]
    }

    /// Writes a `cells` grid of `cell`-pixel squares (see [`cell_color`])
    /// with EXIF `orientation`, set through the metadata `query`.
    fn write_pattern(
        decoder: &Decoder,
        path: &Path,
        container: &GUID,
        query: PCWSTR,
        orientation: u16,
        cells: (u32, u32),
        cell: u32,
    ) {
        let (width, height) = (cells.0 * cell, cells.1 * cell);
        let mut bgr = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let [r, g, b] = cell_color((x / cell, y / cell));
                bgr.extend([b, g, r]);
            }
        }
        // SAFETY: COM calls on live objects owned by this test's thread.
        unsafe {
            let stream = decoder.factory.CreateStream().unwrap();
            stream
                .InitializeFromFilename(&HSTRING::from(path.as_os_str()), 0x4000_0000)
                .unwrap();
            let encoder = decoder
                .factory
                .CreateEncoder(container, std::ptr::null())
                .unwrap();
            encoder
                .Initialize(&stream, WICBitmapEncoderNoCache)
                .unwrap();
            let (mut frame, mut options) = (None, None);
            encoder.CreateNewFrame(&mut frame, &mut options).unwrap();
            let frame = frame.unwrap();
            frame.Initialize(options.as_ref()).unwrap();
            frame.SetSize(width, height).unwrap();
            let mut format = GUID_WICPixelFormat24bppBGR;
            frame.SetPixelFormat(&mut format).unwrap();
            assert_eq!(format, GUID_WICPixelFormat24bppBGR);
            let writer = frame.GetMetadataQueryWriter().unwrap();
            writer
                .SetMetadataByName(query, &PROPVARIANT::from(orientation))
                .unwrap();
            frame.WritePixels(height, width * 3, &bgr).unwrap();
            frame.Commit().unwrap();
            encoder.Commit().unwrap();
        }
    }

    /// A fresh path in the temp folder (removed when dropped).
    struct TempFile(std::path::PathBuf);

    impl TempFile {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("mypaper-wic-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir.join(name))
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// Decodes every orientation of a pattern written to `container` and
    /// checks each cell's centre against the spec. `tolerance` allows for lossy formats.
    fn check_upright(container: &GUID, ext: &str, query: PCWSTR, cell: u32, tolerance: u8) {
        let decoder = Decoder::new().unwrap();
        let cells = (3, 2);
        for orientation in 1..=8u16 {
            let file = TempFile::new(&format!("o{orientation}.{ext}"));
            write_pattern(
                &decoder,
                &file.0,
                container,
                query,
                orientation,
                cells,
                cell,
            );
            let image = decoder.decode(&file.0, budget(Fit::Center)).unwrap();
            let upright = Orientation::from_exif(orientation).unwrap().turn(cells);
            assert_eq!(image.size, (upright.0 * cell, upright.1 * cell));
            for cy in 0..upright.1 {
                for cx in 0..upright.0 {
                    let (px, py) = (cx * cell + cell / 2, cy * cell + cell / 2);
                    let at = ((py * image.size.0 + px) * 4) as usize;
                    let [b, g, r, _] = image.bgra[at..at + 4].try_into().unwrap();
                    let want = cell_color(stored_position(orientation, (cx, cy), cells));
                    assert!(
                        [r, g, b]
                            .iter()
                            .zip(want)
                            .all(|(&a, b)| a.abs_diff(b) <= tolerance),
                        "orientation {orientation}, cell ({cx}, {cy}): got {:?}, want {want:?}",
                        [r, g, b]
                    );
                }
            }
        }
    }

    #[test]
    fn tiff_photos_come_out_upright() {
        check_upright(
            &GUID_ContainerFormatTiff,
            "tif",
            w!("/ifd/{ushort=274}"),
            1,
            0,
        );
    }

    #[test]
    fn jpeg_photos_come_out_upright() {
        // Blocks of 16 px, sampled at their centres, survive JPEG's loss.
        check_upright(
            &GUID_ContainerFormatJpeg,
            "jpg",
            w!("/app1/ifd/{ushort=274}"),
            16,
            24,
        );
    }

    #[test]
    fn scaling_applies_to_the_upright_size() {
        let decoder = Decoder::new().unwrap();
        let file = TempFile::new("scaled.tif");
        write_pattern(
            &decoder,
            &file.0,
            &GUID_ContainerFormatTiff,
            w!("/ifd/{ushort=274}"),
            6,
            (8, 4),
            1,
        );
        // Stored 8x4, upright 4x8; Cover on a 2x2 screen shrinks it to 2x4.
        let screen = Budget {
            screen: (2, 2),
            ..budget(Fit::Cover)
        };
        let image = decoder.decode(&file.0, screen).unwrap();
        assert_eq!(image.size, (2, 4));
        assert_eq!(image.bgra.len(), 2 * 4 * 4);
    }
}
