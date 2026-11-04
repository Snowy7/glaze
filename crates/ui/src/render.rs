//! Software Direct2D on a WIC bitmap: the only renderer, for live surfaces and headless golden
//! images alike. No hardware device is ever created (plan Rule 1); Direct2D's software path
//! may use WARP, which is allowed.

use anyhow::{Context, Result};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
    D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
    D2D1CreateFactory, ID2D1Factory, ID2D1RenderTarget, ID2D1SolidColorBrush,
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
use crate::scene::{DisplayList, Op};
use crate::text::Text;

/// The UI of one thread: the factories, the text cache, and what every surface shares.
/// Create one per UI thread; surfaces and the menu borrow it to draw.
pub struct Ui {
    pub text: Text,
    d2d: ID2D1Factory,
    wic: IWICImagingFactory,
    params: IDWriteRenderingParams,
}

impl Ui {
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
            })
        }
    }

    /// Redraws `area` (cleared first) with the operations of `list` that touch it.
    pub(crate) fn draw(&self, text: &mut Text, list: &DisplayList, area: PixelRect) -> Result<()> {
        let clip = area.to_rect();
        let t = &self.target;
        // SAFETY: drawing calls between BeginDraw and EndDraw on this thread's target; every
        // object passed in is alive for the call.
        unsafe {
            t.BeginDraw();
            t.PushAxisAlignedClip(&d2d(clip), D2D1_ANTIALIAS_MODE_ALIASED);
            t.Clear(Some(&color(Rgba::TRANSPARENT)));
            for op in list.ops.iter().filter(|op| op.bounds().intersects(&clip)) {
                self.op(text, op);
            }
            t.PopAxisAlignedClip();
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
    /// Must run between `BeginDraw` and `EndDraw`.
    unsafe fn op(&self, text: &mut Text, op: &Op) {
        let (t, brush) = (&self.target, &self.brush);
        // SAFETY: per this function's contract.
        unsafe {
            match op {
                Op::Fill {
                    rect,
                    radius,
                    color: c,
                } => {
                    brush.SetColor(&color(*c));
                    self.fill(*rect, *radius);
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
        assert_eq!(old[0], 0, "outside the damage: still the old red at half alpha");
    }
}
