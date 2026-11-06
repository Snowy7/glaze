//! Software Direct2D on a WIC bitmap: the only renderer, for live surfaces and headless golden
//! images alike. No hardware device is ever created (plan Rule 1); Direct2D's software path
//! may use WARP, which is allowed.

use anyhow::{Context, Result};
use std::{cell::RefCell, collections::HashMap, path::Path, sync::Arc};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES,
    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
    D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
    D2D1CreateFactory, ID2D1Bitmap, ID2D1Factory, ID2D1PathGeometry, ID2D1RenderTarget,
    ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_GRID_FIT_MODE_DISABLED, DWRITE_GRID_FIT_MODE_ENABLED, DWRITE_PIXEL_GEOMETRY_FLAT,
    DWRITE_RENDERING_MODE1, DWRITE_RENDERING_MODE1_GDI_CLASSIC, DWRITE_RENDERING_MODE1_NATURAL,
    DWRITE_RENDERING_MODE1_NATURAL_SYMMETRIC, IDWriteFactory3, IDWriteRenderingParams,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap, IWICImagingFactory,
    WICBitmapCacheOnLoad, WICBitmapLockRead, WICRect,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::core::Interface;
use windows_numerics::Vector2;

use crate::geom::{PixelRect, Rect, Rgba};
use crate::scene::{ARC_STEP, DisplayList, Op};
use crate::text::Text;

#[cfg(test)]
#[test]
fn a_sparse_read_keeps_row_offsets_inside_one_nonzero_origin_lock() {
    let mut ui = Ui::new().unwrap();
    let list = DisplayList {
        ops: vec![Op::Image {
            rect: Rect::new(0.0, 0.0, 64.0, 48.0),
            image: Arc::new(Image {
                width: 64,
                height: 48,
                pixels: (0..48)
                    .flat_map(|y| (0..64).flat_map(move |x| [x as u8, y as u8, (x + y) as u8, 255]))
                    .collect(),
            }),
            opacity: 1.0,
            source: None,
        }],
        ..Default::default()
    };
    let canvas = Canvas::new(&ui, 64, 48).unwrap();
    canvas
        .draw(&mut ui.text, &list, PixelRect::full(64, 48))
        .unwrap();
    let expected = render(&mut ui, &list, 64, 48).unwrap();
    let areas = [
        PixelRect {
            x: 3,
            y: 4,
            width: 5,
            height: 6,
        },
        PixelRect {
            x: 31,
            y: 20,
            width: 12,
            height: 10,
        },
    ];
    let mut rows = 0;
    canvas
        .read_regions(&areas, |area, row, bytes| {
            let start = ((area.y + row) * 64 + area.x) as usize * 4;
            assert_eq!(
                bytes,
                &expected.pixels[start..start + area.width as usize * 4]
            );
            rows += 1;
        })
        .unwrap();
    assert_eq!(rows, 16);
}

#[cfg(test)]
#[test]
fn marker_and_retained_clips_are_nested_and_balanced() {
    let mut ui = Ui::new().unwrap();
    let fill = |color| Op::Fill {
        rect: Rect::new(0.0, 0.0, 32.0, 32.0),
        radius: 0.0,
        color: Rgba::rgb8(color, 1.0),
    };
    let list = DisplayList {
        ops: vec![
            Op::Clip {
                rect: Rect::new(0.0, 0.0, 12.0, 12.0),
            },
            Op::Clipped {
                rect: Rect::new(4.0, 4.0, 20.0, 20.0),
                ops: vec![
                    fill([255, 0, 0]),
                    Op::Clip {
                        rect: Rect::new(8.0, 8.0, 20.0, 20.0),
                    },
                    fill([0, 255, 0]),
                    Op::Unclip,
                    Op::Unclip,
                ]
                .into(),
            },
            Op::Unclip,
            Op::Fill {
                rect: Rect::new(24.0, 24.0, 4.0, 4.0),
                radius: 0.0,
                color: Rgba::rgb8([0, 0, 255], 1.0),
            },
        ],
        ..Default::default()
    };
    let image = render(&mut ui, &list, 32, 32).unwrap();
    assert_eq!(image.pixel(5, 5), [0, 0, 255, 255]);
    assert_eq!(image.pixel(10, 10), [0, 255, 0, 255]);
    assert_eq!(image.pixel(16, 16), [0, 0, 0, 0]);
    assert_eq!(image.pixel(25, 25), [255, 0, 0, 255]);
    let translated = render(&mut ui, &list.translated(3.0, 2.0), 40, 40).unwrap();
    for y in 0..32 {
        for x in 0..32 {
            assert_eq!(translated.pixel(x + 3, y + 2), image.pixel(x, y));
        }
    }
}

#[cfg(test)]
#[test]
fn partial_updates_preserve_oversized_elliptical_corners() {
    let mut ui = Ui::new().unwrap();
    let canvas = Canvas::new(&ui, 160, 40).unwrap();
    let frame = |color| DisplayList {
        ops: vec![Op::Fill {
            rect: Rect::new(0.0, 0.0, 160.0, 40.0),
            radius: 80.0,
            color,
        }],
        ..Default::default()
    };
    canvas
        .draw(
            &mut ui.text,
            &frame(Rgba::rgb8([255, 0, 0], 0.5)),
            PixelRect::full(160, 40),
        )
        .unwrap();
    let next = frame(Rgba::rgb8([0, 0, 255], 0.5));
    let dirty = PixelRect {
        x: 30,
        y: 3,
        width: 20,
        height: 10,
    };
    canvas.draw(&mut ui.text, &next, dirty).unwrap();
    let full = render(&mut ui, &next, 160, 40).unwrap();
    canvas
        .read(dirty, |row, bytes| {
            let start = ((dirty.y + row) * 160 + dirty.x) as usize * 4;
            assert_eq!(bytes, &full.pixels[start..start + dirty.width as usize * 4]);
        })
        .unwrap();
}

#[cfg(test)]
#[test]
fn sparse_meter_updates_match_a_complete_render() {
    let mut ui = Ui::new().unwrap();
    let canvas = Canvas::new(&ui, 120, 120).unwrap();
    let frame = |fraction| DisplayList {
        ops: vec![
            Op::Fill {
                rect: Rect::new(0.0, 0.0, 120.0, 120.0),
                radius: 24.0,
                color: Rgba::rgb8([30, 40, 50], 1.0),
            },
            Op::Stroke {
                rect: Rect::new(1.0, 1.0, 118.0, 118.0),
                radius: 24.0,
                width: 1.0,
                color: Rgba::rgb8([80, 90, 100], 1.0),
            },
            Op::Arc {
                center: crate::Point::new(60.0, 60.0),
                radius: 40.0,
                sweep: std::f32::consts::TAU,
                width: 3.0,
                color: Rgba::rgb8([70, 80, 90], 1.0),
            },
            Op::Arc {
                center: crate::Point::new(60.0, 60.0),
                radius: 40.0,
                sweep: fraction * std::f32::consts::TAU,
                width: 3.0,
                color: Rgba::rgb8([130, 190, 255], 1.0),
            },
            Op::Text {
                rect: Rect::new(42.0, 48.0, 36.0, 22.0),
                text: format!("{:.0}%", fraction * 100.0).into(),
                style: crate::TextStyle::new("Segoe UI", 14.0),
                color: Rgba::rgb8([220, 230, 240], 1.0),
            },
        ],
        ..Default::default()
    };
    let full = PixelRect::full(120, 120);
    let mut before = frame(0.20);
    canvas.draw(&mut ui.text, &before, full).unwrap();
    for fraction in [0.21, 0.25, 0.76, 1.0, 0.0, 0.99]
        .into_iter()
        .chain((0..=100).map(|n| n as f32 / 100.0))
        .chain((0..=100).rev().map(|n| n as f32 / 100.0))
    {
        let after = frame(fraction);
        let regions: Vec<_> = after
            .damage_regions(&before)
            .iter()
            .filter_map(|r| r.pixels(120, 120))
            .collect();
        canvas.draw_regions(&mut ui.text, &after, &regions).unwrap();
        let expected = render(&mut ui, &after, 120, 120).unwrap();
        let mut pixels = Vec::new();
        canvas
            .read(full, |_, bytes| pixels.extend_from_slice(bytes))
            .unwrap();
        let changed: Vec<_> = pixels
            .iter()
            .zip(&expected.pixels)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, (a, b))| (i / 4 % 120, i / 4 / 120, *a, *b))
            .collect();
        assert!(
            changed.is_empty(),
            "fraction {fraction}: {} changed bytes, first {:?}",
            changed.len(),
            changed.first()
        );
        before = after;
    }
    assert!(
        canvas.arcs.borrow().len() <= 64,
        "meter geometry stays bounded"
    );
}

/// The UI of one thread: the factories, the text cache, and what every surface shares.
/// Create one per UI thread; surfaces and the menu borrow it to draw.
pub struct Ui {
    pub text: Text,
    d2d: ID2D1Factory,
    wic: IWICImagingFactory,
    params: IDWriteRenderingParams,
}

/// How DirectWrite rasterizes text: always grayscale (layered windows have alpha, so no
/// ClearType), with fixed parameters rather than the machine's ClearType tuning, so golden
/// images and every machine look the same. [`TextQuality::default`] is what every surface
/// uses; the gallery's text lab (`docs/media/native/kit-text.png`) compares the others.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextQuality {
    pub gamma: f32,
    /// Grayscale enhanced contrast: thickens the coverage of light strokes.
    pub contrast: f32,
    pub mode: TextMode,
    /// Hint vertical stems and heights to whole pixels.
    pub grid_fit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextMode {
    /// Subpixel positions, antialiased in both directions (DirectWrite's default above 12 px).
    NaturalSymmetric,
    /// Subpixel positions, antialiased horizontally only.
    Natural,
    /// Whole-pixel advances and full hinting, as GDI.
    GdiClassic,
}

impl Default for TextQuality {
    /// Chosen in the text lab (docs/ui.md, "Text"): Windows' grayscale defaults (gamma 1.8,
    /// grayscale contrast 1.0) with vertical hinting. The old parameters set contrast 0.5
    /// through the ClearType-only field, which drew light text thin and grey on glass.
    fn default() -> Self {
        Self {
            gamma: 1.8,
            contrast: 1.0,
            mode: TextMode::NaturalSymmetric,
            grid_fit: true,
        }
    }
}

impl TextQuality {
    fn params(self, text: &Text) -> Result<IDWriteRenderingParams> {
        let mode: DWRITE_RENDERING_MODE1 = match self.mode {
            TextMode::NaturalSymmetric => DWRITE_RENDERING_MODE1_NATURAL_SYMMETRIC,
            TextMode::Natural => DWRITE_RENDERING_MODE1_NATURAL,
            TextMode::GdiClassic => DWRITE_RENDERING_MODE1_GDI_CLASSIC,
        };
        let grid = if self.grid_fit {
            DWRITE_GRID_FIT_MODE_ENABLED
        } else {
            DWRITE_GRID_FIT_MODE_DISABLED
        };
        // SAFETY: documented factory calls with plain values.
        unsafe {
            let factory: IDWriteFactory3 = text.factory().cast().context("DirectWrite 3")?;
            let params = factory
                .CreateCustomRenderingParams(
                    self.gamma,
                    self.contrast,
                    self.contrast,
                    0.0,
                    DWRITE_PIXEL_GEOMETRY_FLAT,
                    mode,
                    grid,
                )
                .context("text rendering parameters")?;
            Ok(params.cast()?)
        }
    }
}

impl Ui {
    /// Rasterizes the text of canvases made from now on with `quality` (the text lab).
    pub fn set_text_quality(&mut self, quality: TextQuality) -> Result<()> {
        self.params = quality.params(&self.text)?;
        Ok(())
    }

    /// Decode a photo/icon to bounded premultiplied pixels. Keep this result, not the original
    /// full-resolution image, while a widget is visible.
    pub fn decode_image(&self, bytes: &[u8], max_size: (u32, u32)) -> Result<Image> {
        use windows::Win32::Graphics::Imaging::{
            WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
            WICDecodeMetadataCacheOnLoad,
        };
        // SAFETY: WIC consumes this owned stream synchronously. The copied stream buffer stays
        // alive until decoder/converter/scaler are dropped at the end of this block.
        unsafe {
            let stream = self.wic.CreateStream()?;
            stream.InitializeFromMemory(bytes)?;
            let decoder = self.wic.CreateDecoderFromStream(
                &stream,
                std::ptr::null(),
                WICDecodeMetadataCacheOnLoad,
            )?;
            let frame = decoder.GetFrame(0)?;
            let (mut w, mut h) = (0, 0);
            frame.GetSize(&mut w, &mut h)?;
            anyhow::ensure!(
                w > 0 && h > 0 && w <= 32768 && h <= 32768,
                "invalid image dimensions"
            );
            let scale = (max_size.0 as f64 / w as f64)
                .min(max_size.1 as f64 / h as f64)
                .min(1.0);
            let width = (w as f64 * scale).round().max(1.0) as u32;
            let height = (h as f64 * scale).round().max(1.0) as u32;
            let scaler = self.wic.CreateBitmapScaler()?;
            scaler.Initialize(&frame, width, height, WICBitmapInterpolationModeFant)?;
            let converter = self.wic.CreateFormatConverter()?;
            converter.Initialize(
                &scaler,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )?;
            let mut pixels = vec![0; width as usize * height as usize * 4];
            converter.CopyPixels(std::ptr::null(), width * 4, &mut pixels)?;
            Ok(Image {
                width,
                height,
                pixels,
            })
        }
    }
    /// Makes the factories. Initializes COM on this thread (apartment-threaded) if nothing has.
    pub fn new() -> Result<Self> {
        Self::with_text_factory(false)
    }

    /// A private text factory for pictures whose native font resources can be released.
    pub fn isolated() -> Result<Self> {
        Self::with_text_factory(true)
    }

    fn with_text_factory(isolated: bool) -> Result<Self> {
        // SAFETY: documented factory calls on this thread; a COM apartment that already exists
        // (of either kind) is kept, which WIC accepts.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let d2d: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).context("Direct2D")?;
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                    .context("WIC")?;
            let text = if isolated { Text::isolated()? } else { Text::new()? };
            let params = TextQuality::default().params(&text)?;
            Ok(Self {
                text,
                d2d,
                wic,
                params,
            })
        }
    }
}

/// A premultiplied BGRA image, top-down, `width * 4` bytes per row.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    /// Decode a cached app icon or user image to premultiplied BGRA with Windows WIC.
    pub fn load(ui: &Ui, path: &Path) -> Result<Self> {
        use windows::Win32::{
            Foundation::GENERIC_READ,
            Graphics::Imaging::{
                WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnLoad,
            },
        };
        // SAFETY: valid WIC objects and buffers, all confined to this thread.
        unsafe {
            let decoder = ui.wic.CreateDecoderFromFilename(
                &windows::core::HSTRING::from(path.as_os_str()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnLoad,
            )?;
            let frame = decoder.GetFrame(0)?;
            let converter = ui.wic.CreateFormatConverter()?;
            converter.Initialize(
                &frame,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )?;
            let (mut width, mut height) = (0, 0);
            converter.GetSize(&mut width, &mut height)?;
            anyhow::ensure!(
                width > 0 && height > 0 && width <= 4096 && height <= 4096,
                "image dimensions"
            );
            let mut pixels = vec![0; (width * height * 4) as usize];
            converter.CopyPixels(std::ptr::null(), width * 4, &mut pixels)?;
            Ok(Self {
                width,
                height,
                pixels,
            })
        }
    }

    /// The `[b, g, r, a]` of a pixel.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[at],
            self.pixels[at + 1],
            self.pixels[at + 2],
            self.pixels[at + 3],
        ]
    }
}

/// Draws `list` into a new `width`×`height` image exactly as a surface shows it.
pub fn render(ui: &mut Ui, list: &DisplayList, width: u32, height: u32) -> Result<Image> {
    let canvas = Canvas::new(ui, width, height)?;
    let full = PixelRect::full(width, height);
    canvas.draw(&mut ui.text, list, full)?;
    // EndDraw committed the picture to WIC. Release source copies before allocating
    // the returned pixels; a one-shot raster never draws those bitmaps again.
    canvas.images.borrow_mut().clear();
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    canvas.read(full, |row, bytes| {
        let at = row as usize * width as usize * 4;
        pixels[at..at + bytes.len()].copy_from_slice(bytes);
    })?;
    Ok(Image {
        width,
        height,
        pixels,
    })
}

/// A WIC bitmap and the software Direct2D target drawing into it.
pub(crate) struct Canvas {
    bitmap: IWICBitmap,
    target: ID2D1RenderTarget,
    brush: ID2D1SolidColorBrush,
    images: RefCell<HashMap<usize, (Arc<Image>, ID2D1Bitmap)>>,
    arcs: RefCell<HashMap<[u32; 4], ID2D1PathGeometry>>,
    continuous: RefCell<HashMap<[u32; 6], ID2D1PathGeometry>>,
    /// Icon outlines on their 24-unit grid, made on first use.
    icons: RefCell<HashMap<crate::icons::Icon, Option<ID2D1PathGeometry>>>,
    /// Round caps and joins, for icons.
    round: ID2D1StrokeStyle,
}

impl Canvas {
    pub(crate) fn new(ui: &Ui, width: u32, height: u32) -> Result<Self> {
        let properties = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        // SAFETY: documented calls on objects owned by `ui` and by the canvas.
        unsafe {
            let bitmap = ui
                .wic
                .CreateBitmap(
                    width.max(1),
                    height.max(1),
                    &GUID_WICPixelFormat32bppPBGRA,
                    WICBitmapCacheOnLoad,
                )
                .context("WIC bitmap")?;
            let target = ui
                .d2d
                .CreateWicBitmapRenderTarget(&bitmap, &properties)
                .context("software Direct2D target")?;
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            target.SetTextRenderingParams(&ui.params);
            let brush = target.CreateSolidColorBrush(&color(Rgba::TRANSPARENT), None)?;
            use windows::Win32::Graphics::Direct2D::{
                D2D1_CAP_STYLE_ROUND, D2D1_LINE_JOIN_ROUND, D2D1_STROKE_STYLE_PROPERTIES,
            };
            let round = ui.d2d.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    ..Default::default()
                },
                None,
            )?;
            Ok(Self {
                bitmap,
                target,
                brush,
                images: RefCell::new(HashMap::new()),
                arcs: RefCell::new(HashMap::new()),
                continuous: RefCell::new(HashMap::new()),
                icons: RefCell::new(HashMap::new()),
                round,
            })
        }
    }

    /// Redraws `area` (cleared first) with the operations of `list` that touch it.
    pub(crate) fn draw(&self, text: &mut Text, list: &DisplayList, area: PixelRect) -> Result<()> {
        self.draw_regions(text, list, &[area])
    }

    /// Flush Direct2D once for a set of disjoint dashboard changes.
    pub(crate) fn draw_regions(
        &self,
        text: &mut Text,
        list: &DisplayList,
        areas: &[PixelRect],
    ) -> Result<()> {
        // A canvas must not keep page artwork alive after its display list stops using it.
        fn used_images(ops: &[Op], live: &mut std::collections::HashSet<usize>) {
            for op in ops {
                match op {
                    Op::Image { image, .. } | Op::RoundedImage { image, .. } => { live.insert(Arc::as_ptr(image) as usize); }
                    Op::Clipped { ops, .. } => used_images(ops, live),
                    _ => {}
                }
            }
        }
        let mut live = std::collections::HashSet::new();
        used_images(&list.ops, &mut live);
        self.images.borrow_mut().retain(|key, _| live.contains(key));
        let t = &self.target;
        // SAFETY: drawing calls between BeginDraw and EndDraw on this thread's target; every
        // object passed in is alive for the call.
        unsafe {
            t.BeginDraw();
            for area in areas {
                let clip = area.to_rect();
                t.PushAxisAlignedClip(&d2d(clip), D2D1_ANTIALIAS_MODE_ALIASED);
                t.Clear(Some(&color(Rgba::TRANSPARENT)));
                self.ops(text, &list.ops, clip);
                t.PopAxisAlignedClip();
            }
            t.EndDraw(None, None).context("drawing")?;
        }
        Ok(())
    }

    /// Hands each row of `area` (premultiplied BGRA) to `sink`, top to bottom.
    pub(crate) fn read(&self, area: PixelRect, mut sink: impl FnMut(u32, &[u8])) -> Result<()> {
        self.read_regions(&[area], |_, row, bytes| sink(row, bytes))
    }

    /// Pack a dirty area into bounded horizontal bands without copying the full frame.
    pub(crate) fn read_bands(
        &self,
        area: PixelRect,
        budget: usize,
        mut sink: impl FnMut(PixelRect, &[u8]),
    ) -> Result<()> {
        let row_bytes = area.width as usize * 4;
        if row_bytes == 0 || area.height == 0 {
            return Ok(());
        }
        let rows = (budget / row_bytes).max(1).min(area.height as usize);
        let mut pixels = vec![0; rows * row_bytes];
        let mut used = 0;
        self.read(area, |row, bytes| {
            pixels[used * row_bytes..(used + 1) * row_bytes].copy_from_slice(bytes);
            used += 1;
            if used == rows || row + 1 == area.height {
                sink(
                    PixelRect {
                        x: area.x,
                        y: area.y + row + 1 - used as u32,
                        width: area.width,
                        height: used as u32,
                    },
                    &pixels[..used * row_bytes],
                );
                used = 0;
            }
        })
    }

    /// One WIC lock for a sparse frame. Each returned row still contains only its dirty area.
    pub(crate) fn read_regions(
        &self,
        areas: &[PixelRect],
        mut sink: impl FnMut(PixelRect, u32, &[u8]),
    ) -> Result<()> {
        let Some(&first) = areas.first() else {
            return Ok(());
        };
        let (mut left, mut top, mut right, mut bottom) = (
            first.x,
            first.y,
            first.x + first.width,
            first.y + first.height,
        );
        for area in &areas[1..] {
            left = left.min(area.x);
            top = top.min(area.y);
            right = right.max(area.x + area.width);
            bottom = bottom.max(area.y + area.height);
        }
        let rect = WICRect {
            X: left as i32,
            Y: top as i32,
            Width: (right - left) as i32,
            Height: (bottom - top) as i32,
        };
        // SAFETY: the lock covers every area, which lies inside the bitmap; slices stay within
        // the locked buffer (`stride` bytes per row, `width * 4` of them pixels) and are dropped
        // before the lock.
        unsafe {
            let lock = self
                .bitmap
                .Lock(&rect, WICBitmapLockRead.0 as u32)
                .context("locking the bitmap")?;
            let stride = lock.GetStride()? as usize;
            let (mut size, mut data) = (0u32, std::ptr::null_mut());
            lock.GetDataPointer(&mut size, &mut data)?;
            for &area in areas {
                let row_bytes = area.width as usize * 4;
                for row in 0..area.height {
                    let start = data
                        .add((area.y + row - top) as usize * stride + (area.x - left) as usize * 4);
                    sink(area, row, std::slice::from_raw_parts(start, row_bytes));
                }
            }
        }
        Ok(())
    }

    /// # Safety
    /// Runs between BeginDraw/EndDraw. Marker clips are scoped to this slice, including
    /// inside a retained clipped group, so an unmatched Unclip cannot pop a caller's clip.
    unsafe fn ops(&self, text: &mut Text, ops: &[Op], damage: Rect) {
        let mut pushed = 0;
        // SAFETY: the caller owns this draw scope; each pushed clip is balanced below.
        unsafe {
            for op in ops {
                match op {
                    Op::Clip { rect } => {
                        self.target
                            .PushAxisAlignedClip(&d2d(*rect), D2D1_ANTIALIAS_MODE_ALIASED);
                        pushed += 1;
                    }
                    Op::Unclip if pushed > 0 => {
                        self.target.PopAxisAlignedClip();
                        pushed -= 1;
                    }
                    Op::Unclip => {}
                    op if touches(op, damage) => self.op(text, op, damage),
                    _ => {}
                }
            }
            for _ in 0..pushed {
                self.target.PopAxisAlignedClip();
            }
        }
    }

    /// # Safety
    /// Must run between `BeginDraw` and `EndDraw`.
    unsafe fn op(&self, text: &mut Text, op: &Op, damage: Rect) {
        let (t, brush) = (&self.target, &self.brush);
        // SAFETY: per this function's contract.
        unsafe {
            match op {
                Op::Continuous {
                    rect,
                    radius,
                    smoothing,
                    width,
                    color: c,
                } => {
                    use windows::Win32::Graphics::Direct2D::Common::{
                        D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED,
                    };
                    let key = [
                        rect.left,
                        rect.top,
                        rect.right,
                        rect.bottom,
                        *radius,
                        *smoothing,
                    ]
                    .map(f32::to_bits);
                    let mut paths = self.continuous.borrow_mut();
                    if !paths.contains_key(&key)
                        && let Ok(path) = t.GetFactory().and_then(|f| f.CreatePathGeometry())
                        && let Ok(sink) = path.Open()
                    {
                        let points = crate::geom::continuous::outline(*rect, *radius, *smoothing);
                        sink.BeginFigure(
                            Vector2 {
                                X: points[0].x,
                                Y: points[0].y,
                            },
                            D2D1_FIGURE_BEGIN_FILLED,
                        );
                        for p in &points[1..] {
                            sink.AddLine(Vector2 { X: p.x, Y: p.y });
                        }
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        if sink.Close().is_ok() {
                            if paths.len() >= 64 {
                                paths.clear();
                            }
                            paths.insert(key, path);
                        }
                    }
                    if let Some(path) = paths.get(&key) {
                        brush.SetColor(&color(*c));
                        if *width > 0.0 {
                            t.DrawGeometry(path, brush, *width, None);
                        } else {
                            t.FillGeometry(path, brush, None);
                        }
                    }
                }
                Op::Clipped { rect, ops } => {
                    t.PushAxisAlignedClip(&d2d(*rect), D2D1_ANTIALIAS_MODE_ALIASED);
                    let damage = Rect {
                        left: rect.left.max(damage.left),
                        top: rect.top.max(damage.top),
                        right: rect.right.min(damage.right),
                        bottom: rect.bottom.min(damage.bottom),
                    };
                    self.ops(text, ops, damage);
                    t.PopAxisAlignedClip();
                }
                Op::Emphasis {
                    rect,
                    text: string,
                    style,
                    color: c,
                    italic,
                    strike,
                } => {
                    use windows::Win32::Graphics::DirectWrite::{
                        DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL, DWRITE_TEXT_RANGE,
                    };
                    if let Some(layout) = text.layout(string, style) {
                        let range = DWRITE_TEXT_RANGE {
                            startPosition: 0,
                            length: string.encode_utf16().count() as u32,
                        };
                        let _ = layout.SetFontStyle(
                            if *italic {
                                DWRITE_FONT_STYLE_ITALIC
                            } else {
                                DWRITE_FONT_STYLE_NORMAL
                            },
                            range,
                        );
                        let _ = layout.SetStrikethrough(*strike, range);
                        brush.SetColor(&color(*c));
                        t.DrawTextLayout(
                            Vector2 {
                                X: rect.left,
                                Y: rect.top,
                            },
                            layout,
                            brush,
                            D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                        );
                        let _ = layout.SetFontStyle(DWRITE_FONT_STYLE_NORMAL, range);
                        let _ = layout.SetStrikethrough(false, range);
                    }
                }
                Op::Arc {
                    center,
                    radius,
                    sweep,
                    width,
                    color: c,
                } => {
                    use windows::Win32::Graphics::Direct2D::Common::{
                        D2D_SIZE_F, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN,
                    };
                    use windows::Win32::Graphics::Direct2D::{
                        D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_SMALL, D2D1_ELLIPSE,
                        D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    };
                    brush.SetColor(&color(*c));
                    if *sweep >= std::f32::consts::TAU - 0.0001 {
                        t.DrawEllipse(
                            &D2D1_ELLIPSE {
                                point: Vector2 {
                                    X: center.x,
                                    Y: center.y,
                                },
                                radiusX: *radius,
                                radiusY: *radius,
                            },
                            brush,
                            *width,
                            None,
                        );
                    } else if *sweep > 0.0 {
                        let key = [
                            center.x.to_bits(),
                            center.y.to_bits(),
                            radius.to_bits(),
                            sweep.to_bits(),
                        ];
                        let mut arcs = self.arcs.borrow_mut();
                        if !arcs.contains_key(&key)
                            && let Ok(path) = t.GetFactory().and_then(|f| f.CreatePathGeometry())
                            && let Ok(sink) = path.Open()
                        {
                            sink.BeginFigure(
                                Vector2 {
                                    X: center.x,
                                    Y: center.y - radius,
                                },
                                D2D1_FIGURE_BEGIN_HOLLOW,
                            );
                            // Small fixed prefixes reduce meter damage while preserving every
                            // pixel before the changing final segment.
                            let count = (sweep / ARC_STEP).ceil() as u32;
                            for segment in 1..=count {
                                let angle = (segment as f32 * ARC_STEP).min(*sweep);
                                sink.AddArc(&D2D1_ARC_SEGMENT {
                                    point: Vector2 {
                                        X: center.x + radius * angle.sin(),
                                        Y: center.y - radius * angle.cos(),
                                    },
                                    size: D2D_SIZE_F {
                                        width: *radius,
                                        height: *radius,
                                    },
                                    rotationAngle: 0.0,
                                    sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                                    arcSize: D2D1_ARC_SIZE_SMALL,
                                });
                            }
                            sink.EndFigure(D2D1_FIGURE_END_OPEN);
                            if sink.Close().is_ok() {
                                if arcs.len() >= 64 {
                                    arcs.clear();
                                }
                                arcs.insert(key, path);
                            }
                        }
                        if let Some(path) = arcs.get(&key) {
                            t.DrawGeometry(path, brush, *width, None);
                        }
                    }
                }
                Op::Icon {
                    rect,
                    icon,
                    weight,
                    color: c,
                } => self.icon(*rect, *icon, *weight, *c),
                Op::TextField { rect, field, .. } => {
                    if let Err(e) = field.draw(t, *rect) {
                        log::error!("text field: {e:#}");
                    }
                }
                Op::Fill {
                    rect,
                    radius,
                    color: c,
                } => {
                    brush.SetColor(&color(*c));
                    if rounded_contains(rect.inflate(-1.0), (radius - 1.0).max(0.0), damage) {
                        // Meter text is inside the plate's flat interior. Avoid tessellating
                        // the entire rounded outline just to refill this small dirty rectangle.
                        t.FillRectangle(&d2d(damage), brush);
                    } else {
                        self.fill(*rect, *radius);
                    }
                }
                Op::Stroke {
                    rect,
                    radius,
                    width,
                    color: c,
                } => {
                    brush.SetColor(&color(*c));
                    let rounded = D2D1_ROUNDED_RECT {
                        rect: d2d(*rect),
                        radiusX: *radius,
                        radiusY: *radius,
                    };
                    t.DrawRoundedRectangle(&rounded, brush, *width, None);
                }
                Op::Line {
                    from,
                    to,
                    width,
                    color: c,
                } => {
                    brush.SetColor(&color(*c));
                    let point = |p: crate::geom::Point| Vector2 { X: p.x, Y: p.y };
                    t.DrawLine(point(*from), point(*to), brush, *width, None);
                }
                Op::Text {
                    rect,
                    text: string,
                    style,
                    color: c,
                } => {
                    if let Some(layout) = text.layout(string, style) {
                        brush.SetColor(&color(*c));
                        let origin = Vector2 {
                            X: rect.left,
                            Y: rect.top,
                        };
                        t.DrawTextLayout(
                            origin,
                            layout,
                            brush,
                            D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                        );
                    }
                }
                Op::Image {
                    rect,
                    image,
                    opacity,
                    source,
                } => {
                    if let Some(bitmap) = self.bitmap(image) {
                        t.DrawBitmap(
                            &bitmap,
                            Some(&d2d(*rect)),
                            *opacity,
                            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                            source.map(d2d).as_ref().map(|r| r as *const _),
                        );
                    }
                }
                Op::RoundedImage {
                    rect,
                    image,
                    opacity,
                    source,
                    radius,
                } => {
                    use windows::Win32::Graphics::Direct2D::{
                        D2D1_BITMAP_BRUSH_PROPERTIES, D2D1_BRUSH_PROPERTIES, D2D1_EXTEND_MODE_CLAMP,
                    };
                    let Some(bitmap) = self.bitmap(image) else {
                        return;
                    };
                    // The brush maps `source` (bitmap pixels) onto `rect`; the rounded fill
                    // is the clip, antialiased like any other fill.
                    let src = source.unwrap_or(Rect::new(
                        0.0,
                        0.0,
                        image.width as f32,
                        image.height as f32,
                    ));
                    let (sx, sy) = (rect.width() / src.width(), rect.height() / src.height());
                    let transform = windows_numerics::Matrix3x2 {
                        M11: sx,
                        M12: 0.0,
                        M21: 0.0,
                        M22: sy,
                        M31: rect.left - src.left * sx,
                        M32: rect.top - src.top * sy,
                    };
                    if let Ok(fill) = t.CreateBitmapBrush(
                        &bitmap,
                        Some(&D2D1_BITMAP_BRUSH_PROPERTIES {
                            extendModeX: D2D1_EXTEND_MODE_CLAMP,
                            extendModeY: D2D1_EXTEND_MODE_CLAMP,
                            interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        }),
                        Some(&D2D1_BRUSH_PROPERTIES {
                            opacity: *opacity,
                            transform,
                        }),
                    ) {
                        t.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: d2d(*rect),
                                radiusX: *radius,
                                radiusY: *radius,
                            },
                            &fill,
                        );
                    }
                }
                // Applied by `draw`, around the ops they enclose.
                Op::Clip { .. } | Op::Unclip => {}
                Op::Shadow {
                    rect,
                    radius,
                    spread,
                    drop,
                    color: c,
                } => {
                    // Rings two pixels apart, fading outward: no blur effect on a plain target.
                    let steps = (spread / 2.0).floor() as i32;
                    for i in 0..steps {
                        let grow = (i * 2) as f32;
                        let fade = 1.0 - i as f32 / steps as f32;
                        brush.SetColor(&color(c.with_alpha(c.a * fade)));
                        self.fill(rect.inflate(grow).offset(0.0, *drop), radius + grow);
                    }
                }
            }
        }
    }

    /// Builds `icon`'s outline (on its 24-unit grid) as Direct2D geometry.
    fn outline(&self, icon: crate::icons::Icon) -> Option<ID2D1PathGeometry> {
        use crate::icons::Seg;
        use windows::Win32::Graphics::Direct2D::Common::{
            D2D_SIZE_F, D2D1_BEZIER_SEGMENT, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED,
            D2D1_FIGURE_END_OPEN,
        };
        use windows::Win32::Graphics::Direct2D::{
            D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL,
            D2D1_QUADRATIC_BEZIER_SEGMENT, D2D1_SWEEP_DIRECTION_CLOCKWISE,
            D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
        };
        let v = |p: crate::geom::Point| Vector2 { X: p.x, Y: p.y };
        // SAFETY: documented calls on a geometry made here; the sink is closed before use.
        unsafe {
            let path = self.target.GetFactory().ok()?.CreatePathGeometry().ok()?;
            let sink = path.Open().ok()?;
            let mut open = false;
            for seg in icon.segments() {
                match seg {
                    Seg::Move(p) => {
                        if open {
                            sink.EndFigure(D2D1_FIGURE_END_OPEN);
                        }
                        sink.BeginFigure(v(p), D2D1_FIGURE_BEGIN_FILLED);
                        open = true;
                    }
                    Seg::Line(p) => sink.AddLine(v(p)),
                    Seg::Cubic(a, b, p) => sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: v(a),
                        point2: v(b),
                        point3: v(p),
                    }),
                    Seg::Quad(a, p) => sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT {
                        point1: v(a),
                        point2: v(p),
                    }),
                    Seg::Arc {
                        to,
                        rx,
                        ry,
                        rotation,
                        large,
                        sweep,
                    } => sink.AddArc(&D2D1_ARC_SEGMENT {
                        point: v(to),
                        size: D2D_SIZE_F {
                            width: rx,
                            height: ry,
                        },
                        rotationAngle: rotation,
                        sweepDirection: if sweep {
                            D2D1_SWEEP_DIRECTION_CLOCKWISE
                        } else {
                            D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE
                        },
                        arcSize: if large {
                            D2D1_ARC_SIZE_LARGE
                        } else {
                            D2D1_ARC_SIZE_SMALL
                        },
                    }),
                    Seg::Close if open => {
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        open = false;
                    }
                    Seg::Close => {}
                }
            }
            if open {
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            sink.Close().ok()?;
            Some(path)
        }
    }

    /// # Safety
    /// Must run between `BeginDraw` and `EndDraw`.
    unsafe fn icon(&self, rect: Rect, icon: crate::icons::Icon, weight: f32, c: Rgba) {
        let scale = rect.width() / 24.0;
        if scale <= 0.0 {
            return;
        }
        let mut cache = self.icons.borrow_mut();
        let Some(geometry) = cache.entry(icon).or_insert_with(|| self.outline(icon)) else {
            return;
        };
        let t = &self.target;
        // SAFETY: per this function's contract.
        unsafe {
            self.brush.SetColor(&color(c));
            t.SetTransform(&windows_numerics::Matrix3x2 {
                M11: scale,
                M12: 0.0,
                M21: 0.0,
                M22: scale,
                M31: rect.left,
                M32: rect.top,
            });
            if icon.filled() {
                t.FillGeometry(&*geometry, &self.brush, None);
            }
            t.DrawGeometry(&*geometry, &self.brush, weight / scale, &self.round);
            t.SetTransform(&windows_numerics::Matrix3x2::identity());
        }
    }

    /// # Safety
    /// Must run between `BeginDraw` and `EndDraw`.
    /// `image` as a Direct2D bitmap, cached by its allocation (frames share an `Arc`).
    ///
    /// # Safety
    /// Must run between `BeginDraw` and `EndDraw`.
    unsafe fn bitmap(&self, image: &Arc<Image>) -> Option<ID2D1Bitmap> {
        // CreateBitmap reads the whole supplied buffer. Public Image values must be checked
        // before passing a pointer to that FFI call.
        if image.width == 0
            || image.height == 0
            || image
                .width
                .checked_mul(image.height)
                .and_then(|n| n.checked_mul(4))
                .is_none_or(|n| n as usize != image.pixels.len())
        {
            return None;
        }
        let key = Arc::as_ptr(image) as usize;
        let mut cache = self.images.borrow_mut();
        if !cache.contains_key(&key) {
            if cache.len() >= 128 {
                cache.clear();
            }
            let properties = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
            };
            let size = windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U {
                width: image.width,
                height: image.height,
            };
            // SAFETY: the buffer was checked to hold width × height × 4 bytes above.
            if let Ok(bitmap) = unsafe {
                self.target.CreateBitmap(
                    size,
                    Some(image.pixels.as_ptr().cast()),
                    image.width * 4,
                    &properties,
                )
            } {
                cache.insert(key, (image.clone(), bitmap));
            }
        }
        cache.get(&key).map(|(_, bitmap)| bitmap.clone())
    }

    unsafe fn fill(&self, rect: Rect, radius: f32) {
        // SAFETY: per this function's contract.
        unsafe {
            if radius > 0.0 {
                let rounded = D2D1_ROUNDED_RECT {
                    rect: d2d(rect),
                    radiusX: radius,
                    radiusY: radius,
                };
                self.target.FillRoundedRectangle(&rounded, &self.brush);
            } else {
                self.target.FillRectangle(&d2d(rect), &self.brush);
            }
        }
    }
}

/// Bounding boxes include a ring's empty middle and a frame's empty interior. Avoid sending
/// those unchanged outlines through Direct2D when only a meter's central label changed.
fn touches(op: &Op, damage: Rect) -> bool {
    if !op.bounds().intersects(&damage) {
        return false;
    }
    match op {
        Op::Arc {
            center,
            radius,
            width,
            ..
        } => {
            let near_x = center.x - center.x.clamp(damage.left, damage.right);
            let near_y = center.y - center.y.clamp(damage.top, damage.bottom);
            let far_x = (center.x - damage.left)
                .abs()
                .max((center.x - damage.right).abs());
            let far_y = (center.y - damage.top)
                .abs()
                .max((center.y - damage.bottom).abs());
            let outer = radius + width / 2.0 + 2.0;
            let inner = (radius - width / 2.0 - 2.0).max(0.0);
            near_x * near_x + near_y * near_y <= outer * outer
                && far_x * far_x + far_y * far_y >= inner * inner
        }
        Op::Stroke {
            rect,
            radius,
            width,
            ..
        } => {
            let inset = width / 2.0 + 2.0;
            !rounded_contains(rect.inflate(-inset), (radius - inset).max(0.0), damage)
        }
        _ => true,
    }
}

/// Convex rounded bounds contain a box when they contain all four corners. Callers inset
/// the shape so its antialiased edge stays on the normal rendering path.
fn rounded_contains(rect: Rect, radius: f32, damage: Rect) -> bool {
    if rect.is_empty()
        || damage.left < rect.left
        || damage.top < rect.top
        || damage.right > rect.right
        || damage.bottom > rect.bottom
    {
        return false;
    }
    // Direct2D clamps oversized X/Y radii independently, producing elliptical corners.
    // Leave that less common case to Direct2D rather than assume a circular interior.
    if radius * 2.0 > rect.width().min(rect.height()) {
        return false;
    }
    let radius = radius.max(0.0);
    [damage.left, damage.right].into_iter().all(|x| {
        [damage.top, damage.bottom].into_iter().all(|y| {
            let dx = x - x.clamp(rect.left + radius, rect.right - radius);
            let dy = y - y.clamp(rect.top + radius, rect.bottom - radius);
            dx * dx + dy * dy <= radius * radius
        })
    })
}

fn d2d(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

fn color(c: Rgba) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_ops_preserve_premultiplied_alpha_and_only_cover_their_rect() {
        let mut ui = Ui::new().unwrap();
        let bitmap = Arc::new(Image {
            width: 1,
            height: 1,
            pixels: vec![0, 0, 128, 128],
        });
        let list = DisplayList {
            ops: vec![Op::Image {
                rect: Rect::new(1.0, 1.0, 2.0, 2.0),
                image: bitmap,
                opacity: 1.0,
                source: None,
            }],
            ..Default::default()
        };
        let image = render(&mut ui, &list, 4, 4).unwrap();
        assert_eq!(image.pixel(0, 0), [0, 0, 0, 0]);
        let [b, g, r, a] = image.pixel(1, 1);
        assert_eq!((b, g), (0, 0));
        assert!(r.abs_diff(128) <= 1 && a.abs_diff(128) <= 1);
    }

    #[test]
    fn a_rounded_image_clears_its_corners_and_maps_its_source_crop() {
        let mut ui = Ui::new().unwrap();
        // Left half red, right half blue; the op shows only the right half, rounded.
        let pixels = (0..8 * 8)
            .flat_map(|i| {
                if i % 8 < 4 {
                    [0, 0, 255, 255]
                } else {
                    [255, 0, 0, 255]
                }
            })
            .collect();
        let list = DisplayList {
            ops: vec![Op::RoundedImage {
                rect: Rect::new(0.0, 0.0, 32.0, 32.0),
                image: Arc::new(Image {
                    width: 8,
                    height: 8,
                    pixels,
                }),
                opacity: 1.0,
                source: Some(Rect::new(4.0, 0.0, 4.0, 8.0)),
                radius: 10.0,
            }],
            ..Default::default()
        };
        let image = render(&mut ui, &list, 32, 32).unwrap();
        assert_eq!(
            image.pixel(0, 0),
            [0, 0, 0, 0],
            "the corner is clipped clean"
        );
        assert_eq!(
            image.pixel(16, 16),
            [255, 0, 0, 255],
            "the centre shows the cropped half"
        );
        assert_eq!(
            image.pixel(6, 16),
            [255, 0, 0, 255],
            "the crop's left side is blue too"
        );
    }

    #[test]
    fn a_fill_lands_premultiplied_and_only_the_damaged_area_is_redrawn() {
        let mut ui = Ui::new().unwrap();
        let red = |x| Op::Fill {
            rect: Rect::new(x, 0.0, 4.0, 4.0),
            radius: 0.0,
            color: Rgba::new(1.0, 0.0, 0.0, 0.5),
        };
        let list = DisplayList {
            ops: vec![red(0.0), red(8.0)],
            ..Default::default()
        };
        let canvas = Canvas::new(&ui, 12, 4).unwrap();
        canvas
            .draw(&mut ui.text, &list, PixelRect::full(12, 4))
            .unwrap();
        // Change both fills but redraw only the left one's pixels: the right one keeps the
        // old colour, which proves the clip.
        let blue = |x| Op::Fill {
            rect: Rect::new(x, 0.0, 4.0, 4.0),
            radius: 0.0,
            color: Rgba::new(0.0, 0.0, 1.0, 1.0),
        };
        let next = DisplayList {
            ops: vec![blue(0.0), blue(8.0)],
            ..Default::default()
        };
        canvas
            .draw(&mut ui.text, &next, PixelRect::full(4, 4))
            .unwrap();
        let mut rows = Vec::new();
        canvas
            .read(PixelRect::full(12, 4), |_, bytes| rows.push(bytes.to_vec()))
            .unwrap();
        let px = |x: usize| &rows[1][x * 4..x * 4 + 4];
        assert_eq!(px(1), [255, 0, 0, 255], "redrawn: opaque blue (BGRA)");
        assert_eq!(px(5), [0, 0, 0, 0], "between the fills: transparent");
        let old = px(9);
        assert!(old[2] > 120 && old[2] < 135 && old[3] > 120 && old[3] < 135);
        assert_eq!(
            old[0], 0,
            "outside the damage: still the old red at half alpha"
        );
    }
}

#[test]
fn canvas_releases_pictures_that_left_the_display_list() {
    let mut ui = Ui::new().unwrap();
    let canvas = Canvas::new(&ui, 8, 8).unwrap();
    let image = Arc::new(Image { width: 8, height: 8, pixels: vec![255; 8 * 8 * 4] });
    let old = Arc::downgrade(&image);
    let mut list = DisplayList::default();
    list.ops.push(Op::RoundedImage { rect: Rect::new(0.0,0.0,8.0,8.0), image, opacity:1.0, source:None, radius:1.0 });
    canvas.draw(&mut ui.text, &list, PixelRect::full(8,8)).unwrap();
    drop(list);
    assert!(old.upgrade().is_some(), "canvas retained its live bitmap");
    canvas.draw(&mut ui.text, &DisplayList::default(), PixelRect::full(8,8)).unwrap();
    assert!(old.upgrade().is_none(), "the previous page's pixels were freed");
}

#[test]
#[cfg(test)]
fn bounded_canvas_bands_preserve_partial_damage_and_the_last_short_band() {
    let mut ui = Ui::new().unwrap();
    let image = Arc::new(Image {
        width: 64,
        height: 48,
        pixels: (0..48)
            .flat_map(|y| (0..64).flat_map(move |x| [x, y, x + y, 255]))
            .collect(),
    });
    let canvas = Canvas::new(&ui, 64, 48).unwrap();
    let list = DisplayList {
        ops: vec![Op::Image {
            rect: Rect::new(0.0, 0.0, 64.0, 48.0),
            image: image.clone(),
            opacity: 1.0,
            source: None,
        }],
        ..Default::default()
    };
    canvas
        .draw(&mut ui.text, &list, PixelRect::full(64, 48))
        .unwrap();
    let area = PixelRect {
        x: 3,
        y: 4,
        width: 31,
        height: 20,
    };
    let mut heights = Vec::new();
    canvas
        .read_bands(area, 31 * 4 * 7 + 5, |band, bytes| {
            heights.push(band.height);
            assert_eq!(band.x, area.x);
            assert_eq!(band.width, area.width);
            for row in 0..band.height {
                let source = ((band.y + row) * 64 + band.x) as usize * 4;
                let packed = row as usize * band.width as usize * 4;
                assert_eq!(
                    &bytes[packed..packed + 31 * 4],
                    &image.pixels[source..source + 31 * 4]
                );
            }
        })
        .unwrap();
    assert_eq!(heights, [7, 7, 6]);
}
