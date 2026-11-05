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
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1Bitmap,
    ID2D1Factory, ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_PIXEL_GEOMETRY_FLAT, DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC, IDWriteRenderingParams,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap, IWICImagingFactory,
    WICBitmapCacheOnLoad, WICBitmapLockRead, WICRect,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows_numerics::Vector2;

use crate::geom::{PixelRect, Rect, Rgba};
use crate::scene::{ARC_STEP, DisplayList, Op};
use crate::text::Text;

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
}

/// The UI of one thread: the factories, the text cache, and what every surface shares.
/// Create one per UI thread; surfaces and the menu borrow it to draw.
pub struct Ui {
    pub text: Text,
    d2d: ID2D1Factory,
    wic: IWICImagingFactory,
    params: IDWriteRenderingParams,
}

impl Ui {
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
        // SAFETY: documented factory calls on this thread; a COM apartment that already exists
        // (of either kind) is kept, which WIC accepts.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let d2d: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).context("Direct2D")?;
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                    .context("WIC")?;
            let text = Text::new()?;
            // Fixed grayscale parameters, not the user's ClearType tuning: layered windows have
            // alpha (no ClearType), and golden images must not depend on the machine's settings.
            let params = text
                .factory()
                .CreateCustomRenderingParams(
                    1.8,
                    0.5,
                    0.0,
                    DWRITE_PIXEL_GEOMETRY_FLAT,
                    DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
                )
                .context("text rendering parameters")?;
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
            Ok(Self {
                bitmap,
                target,
                brush,
                images: RefCell::new(HashMap::new()),
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
        let rect = WICRect {
            X: area.x as i32,
            Y: area.y as i32,
            Width: area.width as i32,
            Height: area.height as i32,
        };
        // SAFETY: the lock covers `area`, which lies inside the bitmap; the slices stay within
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
            let row_bytes = area.width as usize * 4;
            for row in 0..area.height {
                let start = data.add(row as usize * stride);
                sink(row, std::slice::from_raw_parts(start, row_bytes));
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
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
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
                    } else if *sweep > 0.0
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
                            t.DrawGeometry(&path, brush, *width, None);
                        }
                    }
                }
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
                        t.DrawTextLayout(origin, layout, brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
                    }
                }
                Op::Image {
                    rect,
                    image,
                    opacity,
                    source,
                } => {
                    // CreateBitmap reads the whole supplied buffer. Public Image values must be
                    // checked before passing a pointer to that FFI call.
                    if image.width == 0
                        || image.height == 0
                        || image
                            .width
                            .checked_mul(image.height)
                            .and_then(|n| n.checked_mul(4))
                            .is_none_or(|n| n as usize != image.pixels.len())
                    {
                        return;
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
                        if let Ok(bitmap) = t.CreateBitmap(
                            size,
                            Some(image.pixels.as_ptr().cast()),
                            image.width * 4,
                            &properties,
                        ) {
                            cache.insert(key, (image.clone(), bitmap));
                        }
                    }
                    if let Some((_, bitmap)) = cache.get(&key) {
                        t.DrawBitmap(
                            bitmap,
                            Some(&d2d(*rect)),
                            *opacity,
                            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                            source.map(d2d).as_ref().map(|r| r as *const _),
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

    /// # Safety
    /// Must run between `BeginDraw` and `EndDraw`.
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
