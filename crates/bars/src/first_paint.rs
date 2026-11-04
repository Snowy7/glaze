//! The bars' native first paint (docs/performance.md, "Native first paint").
//!
//! WebView2 takes hundreds of milliseconds to start, and a bar's window shows nothing until its
//! page reports what it drew (the window region). So that a bar is there from the first moment,
//! each bar remembers where its page drew last time (`%LOCALAPPDATA%\mypaper\bars-first-paint.json`)
//! and, on the next start, a plain layered window paints those parts in the bar's current look
//! (the page's own background: the tint at its opacity, rounded like the page) over the
//! wallpaper backdrop when there is one. It goes away once the page has painted and set its
//! region, or after [`EXPIRE`].
//!
//! The last values of the modules whose providers answer late (media, tray, stats) are kept in
//! the same file and sent to a page that asks before the providers have reported, so its first
//! render has real data; the providers' first report replaces them.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Once;
use std::time::{Duration, Instant};

use mypaper_bars::BarsConfig;
use mypaper_desktop::{Monitor, Rect, startup};
use mypaper_theme::Surface;
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_TOPMOST, RegisterClassExW,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos, ShowWindow, ULW_ALPHA,
    UpdateLayeredWindow, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{GUID, HSTRING, PCWSTR, w};

use crate::bar::Bar;
use crate::bridge::{BarKind, Topic};
use crate::look::Look;

const FILE: &str = "bars-first-paint.json";
const CLASS: PCWSTR = w!("mypaper.bars.first-paint");
/// A page that hasn't painted by then isn't coming soon (WebView2 missing or failing); the
/// stand-in goes rather than pose as a working bar.
pub const EXPIRE: Duration = Duration::from_secs(15);

/// Where a bar's page drew, the last time it laid out after loading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Shape {
    kind: String,
    device: String,
    /// The bar window's rect then (px). A bar placed differently now isn't painted from it.
    rect: [i32; 4],
    scale: f64,
    /// The page's visible parts (DIPs from the window's top-left).
    region: Vec<[f64; 4]>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    #[serde(default)]
    shapes: Vec<Shape>,
    /// The last message per late topic (`media`, `tray`, `stats`), as sent to a page.
    #[serde(default)]
    values: BTreeMap<String, String>,
}

/// Topics whose providers answer after the page usually asks.
fn late(topic: Topic) -> bool {
    matches!(topic, Topic::Media | Topic::Tray | Topic::Stats)
}

fn kind_name(kind: BarKind) -> &'static str {
    match kind {
        BarKind::Top => "top",
        BarKind::Dock => "dock",
    }
}

fn rect_array(rect: Rect) -> [i32; 4] {
    [rect.x, rect.y, rect.width, rect.height]
}

/// A bar by what it is rather than by id: (kind, monitor device). Stand-ins go up before any
/// bar exists, so they're keyed by this.
type Key = (&'static str, String);

fn key_of(bar: &Bar) -> Key {
    (kind_name(bar.kind), bar.monitor.device.clone())
}

/// The bars' first-paint state: the cache, the stand-in windows, and which late topics have
/// reported since the start.
pub struct FirstPaint {
    path: Option<PathBuf>,
    cache: Cache,
    /// Stand-ins on screen.
    splashes: HashMap<Key, Splash>,
    /// Bars that got a stand-in (or were found to need none) already.
    handled: HashSet<Key>,
    /// Which bar is which, once they exist.
    keys: HashMap<u32, Key>,
    /// Bars whose page painted since it loaded.
    painted: HashSet<u32>,
    /// Bars whose page set a region since it loaded (its shape is recorded then).
    laid_out: HashSet<u32>,
    reported: HashSet<Topic>,
    started: Instant,
}

impl FirstPaint {
    pub fn load() -> Self {
        let path = mypaper_bars::files::local(FILE).ok();
        let cache = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            path,
            cache,
            splashes: HashMap::new(),
            handled: HashSet::new(),
            keys: HashMap::new(),
            painted: HashSet::new(),
            laid_out: HashSet::new(),
            reported: HashSet::new(),
            started: Instant::now(),
        }
    }

    /// Puts up the stand-in of every bar `config` puts on `monitors` (but not on `covered`, the
    /// monitor of a fullscreen app) whose shape is cached. Called as soon as the host knows the
    /// look, before the bars reserve their space (several synchronous round trips to Explorer);
    /// [`sync`](Self::sync) then drops any that the real bar doesn't land on.
    pub fn show_early(
        &mut self,
        monitors: &[Monitor],
        config: &BarsConfig,
        covered: Option<&str>,
        look: impl Fn(BarKind) -> Look,
    ) {
        let (top, dock) = (&config.top, &config.dock);
        let wanted = monitors
            .iter()
            .filter(|m| covered != Some(m.device.as_str()))
            .flat_map(|m| {
                [
                    (
                        BarKind::Top,
                        top.enabled && top.monitors.includes(&m.device, m.primary),
                    ),
                    (
                        BarKind::Dock,
                        dock.enabled && dock.monitors.includes(&m.device, m.primary),
                    ),
                ]
                .into_iter()
                .filter(|(_, on)| *on)
                .map(|(kind, _)| (kind, m.clone()))
            });
        self.show_cached(wanted.collect::<Vec<_>>(), look);
    }

    /// Puts up the stand-in of every bar in `wanted` (kind and monitor) that doesn't have one
    /// yet and whose shape is cached for a monitor at the same scale.
    fn show_cached(
        &mut self,
        wanted: impl IntoIterator<Item = (BarKind, Monitor)>,
        look: impl Fn(BarKind) -> Look,
    ) {
        for (kind, monitor) in wanted {
            let key = (kind_name(kind), monitor.device.clone());
            if !self.handled.insert(key.clone()) {
                continue;
            }
            let scale = f64::from(monitor.dpi) / 96.0;
            let shape = self
                .cache
                .shapes
                .iter()
                .find(|s| s.kind == key.0 && s.device == key.1 && s.scale == scale);
            if let Some(splash) = shape.and_then(|s| Splash::show(s, &monitor, &look(kind))) {
                startup::ready("bars", "native_paint_ms");
                self.splashes.insert(key, splash);
            }
        }
    }

    /// Follows the real bars: a stand-in stays only over a visible bar placed where it was
    /// cached; a bar that shows up later gets one too, once.
    pub fn sync(&mut self, bars: &[Bar], look: impl Fn(BarKind) -> Look) {
        self.keys = bars.iter().map(|b| (b.id, key_of(b))).collect();
        self.splashes.retain(|key, splash| {
            bars.iter()
                .any(|b| key_of(b) == *key && !b.hidden && splash.rect == rect_array(b.rect))
        });
        let visible = bars.iter().filter(|b| !b.hidden);
        self.show_cached(visible.map(|b| (b.kind, b.monitor.clone())), look);
    }

    /// The page of bar `id` (re)loaded: its next region is its new shape.
    pub fn hello(&mut self, id: u32) {
        self.laid_out.remove(&id);
        self.painted.remove(&id);
    }

    /// Bar `id`'s page answered the paint probe.
    pub fn painted(&mut self, id: u32) {
        self.painted.insert(id);
        self.reveal(id);
    }

    /// Bar `bar`'s page set its region: the first one after a load is its shape.
    pub fn region(&mut self, bar: &Bar, rects: &[[f64; 4]]) {
        if rects.is_empty() || !self.laid_out.insert(bar.id) {
            return;
        }
        let shape = Shape {
            kind: kind_name(bar.kind).into(),
            device: bar.monitor.device.clone(),
            rect: rect_array(bar.rect),
            scale: bar.scale(),
            region: rects.to_vec(),
        };
        let shapes = &mut self.cache.shapes;
        let same = |s: &Shape| s.kind == shape.kind && s.device == shape.device;
        if shapes.iter().find(|s| same(s)) != Some(&shape) {
            shapes.retain(|s| !same(s));
            shapes.push(shape);
            self.save();
        }
        self.reveal(bar.id);
    }

    /// The stand-in goes once the page both painted and set its region (the window shows
    /// nothing before it has a region).
    fn reveal(&mut self, id: u32) {
        if self.painted.contains(&id)
            && self.laid_out.contains(&id)
            && let Some(key) = self.keys.get(&id)
        {
            self.splashes.remove(key);
        }
    }

    /// Drops stand-ins that outlived [`EXPIRE`].
    pub fn expire(&mut self, now: Instant) {
        if !self.splashes.is_empty() && now.duration_since(self.started) > EXPIRE {
            log::warn!("the bars didn't paint within {EXPIRE:?}; removing their stand-ins");
            self.splashes.clear();
        }
    }

    /// A late topic's provider reported: live values from now on.
    pub fn reported(&mut self, topic: Topic) {
        self.reported.insert(topic);
    }

    /// The cached message for `topic` while its provider hasn't reported yet.
    pub fn stand_in(&self, topic: Topic) -> Option<&str> {
        (late(topic) && !self.reported.contains(&topic))
            .then(|| self.cache.values.get(topic.event()))
            .flatten()
            .map(String::as_str)
    }

    /// Remembers a live message of a late topic (saved with the next shape and on exit).
    pub fn sent(&mut self, topic: Topic, message: &str) {
        if late(topic) && self.reported.contains(&topic) {
            self.cache
                .values
                .insert(topic.event().to_owned(), message.to_owned());
        }
    }

    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        let saved = serde_json::to_string(&self.cache)
            .map_err(anyhow::Error::from)
            .and_then(|text| mypaper_bars::files::write_atomic(path, text.as_bytes()));
        if let Err(e) = saved {
            log::warn!("couldn't save {FILE}: {e:#}");
        }
    }
}

/// A stand-in window: click-through, never activated, topmost, painted once.
struct Splash {
    hwnd: HWND,
    /// The bar window rect it stands in for.
    rect: [i32; 4],
}

impl Splash {
    /// Paints `shape` in `look` where its bar's window was, on `monitor`. `None` when there's
    /// nothing to paint.
    fn show(shape: &Shape, monitor: &Monitor, look: &Look) -> Option<Self> {
        if look.surface == Surface::None || look.opacity <= 0.0 {
            return None;
        }
        let scale = shape.scale;
        let rects: Vec<[f64; 4]> = shape
            .region
            .iter()
            .map(|[x, y, w, h]| [x * scale, y * scale, w * scale, h * scale])
            .collect();
        // Only the drawn parts' bounding box gets a window (not the room for flyouts).
        let left = rects.iter().map(|r| r[0]).fold(f64::MAX, f64::min).floor();
        let top = rects.iter().map(|r| r[1]).fold(f64::MAX, f64::min).floor();
        let right = rects
            .iter()
            .map(|r| r[0] + r[2])
            .fold(f64::MIN, f64::max)
            .ceil();
        let bottom = rects
            .iter()
            .map(|r| r[1] + r[3])
            .fold(f64::MIN, f64::max)
            .ceil();
        let (width, height) = ((right - left) as i32, (bottom - top) as i32);
        if width <= 0 || height <= 0 || width > 16384 || height > 16384 {
            return None;
        }
        let origin = (shape.rect[0] + left as i32, shape.rect[1] + top as i32);
        let local: Vec<[f64; 4]> = rects
            .iter()
            .map(|[x, y, w, h]| [x - left, y - top, *w, *h])
            .collect();
        let backdrop = Backdrop::load(&monitor.device, monitor.rect);
        let backdrop_at = |px: i32, py: i32| {
            backdrop
                .as_ref()
                .and_then(|b| b.at(origin.0 + px, origin.1 + py))
        };
        let pixels = paint(
            (width, height),
            &local,
            f64::from(look.radius) * scale,
            look.tint.0,
            look.opacity.clamp(0.0, 1.0),
            backdrop_at,
        );
        let splash = Self {
            hwnd: create_window()?,
            rect: shape.rect,
        };
        // SAFETY: our own window; `pixels` covers width x height.
        unsafe { present(splash.hwnd, origin, (width, height), &pixels) }
            .inspect_err(|e| log::warn!("no native first paint: {e}"))
            .ok()?;
        Some(splash)
    }
}

impl Drop for Splash {
    fn drop(&mut self) {
        // SAFETY: our own window, destroyed on its thread.
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}

/// The stand-in has nothing to handle: everything is the default.
unsafe extern "system" fn splash_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: default handling of a message we were called with.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn create_window() -> Option<HWND> {
    static REGISTER: Once = Once::new();
    // SAFETY: plain Win32 registration and creation with a 'static class name and window
    // procedure.
    unsafe {
        let instance = GetModuleHandleW(None).ok()?.into();
        REGISTER.call_once(|| {
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(splash_proc),
                hInstance: instance,
                lpszClassName: CLASS,
                ..Default::default()
            };
            RegisterClassExW(&class);
        });
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            CLASS,
            w!("mypaper bar (starting)"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )
        .ok()
    }
}

/// Puts premultiplied BGRA `pixels` on the layered window and shows it.
///
/// # Safety
/// `hwnd` must be a live layered window of this thread; `pixels.len() == width * height`.
unsafe fn present(
    hwnd: HWND,
    (x, y): (i32, i32),
    (width, height): (i32, i32),
    pixels: &[u32],
) -> windows::core::Result<()> {
    // SAFETY: GDI objects created and released here; the DIB's memory is written within its
    // size; the caller guarantees the window and the pixel count.
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let result = CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            .and_then(|bitmap| {
                std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u32>(), pixels.len());
                let old = SelectObject(memory, bitmap.into());
                let blend = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                let shown = UpdateLayeredWindow(
                    hwnd,
                    Some(screen),
                    Some(&POINT { x, y }),
                    Some(&SIZE {
                        cx: width,
                        cy: height,
                    }),
                    Some(memory),
                    Some(&POINT::default()),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                );
                SelectObject(memory, old);
                let _ = DeleteObject(bitmap.into());
                shown
            });
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        result?;
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        Ok(())
    }
}

/// Premultiplied BGRA pixels of `rects` (px, in the image) filled with `tint` at `opacity`,
/// corners rounded by `radius` (antialiased); over `backdrop` (opaque, screen colours) where it
/// has a pixel. Everything else is transparent.
fn paint(
    (width, height): (i32, i32),
    rects: &[[f64; 4]],
    radius: f64,
    tint: [u8; 3],
    opacity: f32,
    backdrop: impl Fn(i32, i32) -> Option<[u8; 3]>,
) -> Vec<u32> {
    let mut pixels = vec![0u32; (width * height) as usize];
    for py in 0..height {
        for px in 0..width {
            let (cx, cy) = (f64::from(px) + 0.5, f64::from(py) + 0.5);
            let coverage = rects
                .iter()
                .map(|&rect| rounded_coverage(rect, radius, cx, cy))
                .fold(0.0, f64::max) as f32;
            if coverage <= 0.0 {
                continue;
            }
            let (rgb, alpha) = match backdrop(px, py) {
                Some(under) => {
                    let mix =
                        |t: u8, u: u8| f32::from(t) * opacity + f32::from(u) * (1.0 - opacity);
                    ([0, 1, 2].map(|i| mix(tint[i], under[i])), 1.0)
                }
                None => (tint.map(f32::from), opacity),
            };
            let a = alpha * coverage;
            let channel = |c: f32| (c * a).round().clamp(0.0, 255.0) as u32;
            let [r, g, b] = rgb;
            pixels[(py * width + px) as usize] =
                channel(255.0) << 24 | channel(r) << 16 | channel(g) << 8 | channel(b);
        }
    }
    pixels
}

/// How much of the pixel centred at (`x`, `y`) a rect with rounded corners covers, 0 to 1.
fn rounded_coverage([left, top, width, height]: [f64; 4], radius: f64, x: f64, y: f64) -> f64 {
    let (right, bottom) = (left + width, top + height);
    let r = radius.clamp(0.0, width.min(height) / 2.0);
    // Distance outside the rect shrunk by r, measured against r (signed-distance style).
    let nearest_x = x.clamp(left + r, right - r);
    let nearest_y = y.clamp(top + r, bottom - r);
    let outside = ((x - nearest_x).powi(2) + (y - nearest_y).powi(2)).sqrt() - r;
    let edge = (x - left).min(right - x).min(y - top).min(bottom - y);
    if r > 0.0 {
        (0.5 - outside).clamp(0.0, 1.0)
    } else {
        (edge + 0.5).clamp(0.0, 1.0)
    }
}

/// The wallpaper backdrop of a monitor, when one was saved
/// (`%LOCALAPPDATA%\mypaper\backdrop\<DISPLAYn>.jpg`), decoded once.
struct Backdrop {
    monitor: Rect,
    size: (u32, u32),
    /// Tightly packed BGRA.
    pixels: Vec<u8>,
}

impl Backdrop {
    fn load(device: &str, monitor: Rect) -> Option<Self> {
        let name = device.trim_start_matches(r"\\.\");
        let path = mypaper_bars::files::local("backdrop")
            .ok()?
            .join(format!("{name}.jpg"));
        if !path.is_file() {
            return None;
        }
        decode(&path, monitor)
            .inspect_err(|e| log::warn!("backdrop {}: {e}", path.display()))
            .ok()
    }

    /// The backdrop's colour under screen point (`x`, `y`).
    fn at(&self, x: i32, y: i32) -> Option<[u8; 3]> {
        let Rect {
            x: mx,
            y: my,
            width,
            height,
        } = self.monitor;
        if width <= 0 || height <= 0 {
            return None;
        }
        let (w, h) = self.size;
        let bx = (i64::from(x - mx) * i64::from(w) / i64::from(width)).clamp(0, i64::from(w) - 1);
        let by = (i64::from(y - my) * i64::from(h) / i64::from(height)).clamp(0, i64::from(h) - 1);
        let i = ((by * i64::from(w) + bx) * 4) as usize;
        let p = self.pixels.get(i..i + 3)?;
        Some([p[2], p[1], p[0]])
    }
}

fn decode(path: &std::path::Path, monitor: Rect) -> windows::core::Result<Backdrop> {
    // SAFETY: WIC calls on this (COM-initialized) thread with live objects; the pixel buffer
    // matches the size and stride passed.
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let decoder = factory.CreateDecoderFromFilename(
            &HSTRING::from(path.as_os_str()),
            None,
            windows::Win32::Foundation::GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )?;
        let frame = decoder.GetFrame(0)?;
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppBGRA as *const GUID,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        let (mut w, mut h) = (0, 0);
        converter.GetSize(&mut w, &mut h)?;
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        converter.CopyPixels(std::ptr::null(), w * 4, &mut pixels)?;
        Ok(Backdrop {
            monitor,
            size: (w, h),
            pixels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_is_full_inside_none_outside_and_rounded_at_corners() {
        let rect = [0.0, 0.0, 100.0, 32.0];
        assert_eq!(rounded_coverage(rect, 0.0, 50.5, 16.5), 1.0);
        assert_eq!(rounded_coverage(rect, 0.0, 150.5, 16.5), 0.0);
        // Square corner pixel: covered; rounded (r = 10): the very corner isn't.
        assert_eq!(rounded_coverage(rect, 0.0, 0.5, 0.5), 1.0);
        assert_eq!(rounded_coverage(rect, 10.0, 0.5, 0.5), 0.0);
        assert_eq!(rounded_coverage(rect, 10.0, 10.5, 0.5), 1.0);
    }

    #[test]
    fn paints_the_tint_premultiplied_or_over_the_backdrop() {
        let rects = [[0.0, 0.0, 2.0, 1.0]];
        // 50% white over nothing: premultiplied (a = 128, rgb = 128).
        let glass = paint((3, 1), &rects, 0.0, [255; 3], 0.5, |_, _| None);
        assert_eq!(glass[0], 0x80_80_80_80);
        assert_eq!(glass[2], 0, "outside the region stays transparent");
        // Over a black backdrop: opaque mid-grey.
        let solid = paint((3, 1), &rects, 0.0, [255; 3], 0.5, |_, _| Some([0; 3]));
        assert_eq!(solid[1], 0xFF_80_80_80);
    }

    #[test]
    fn stand_in_values_only_until_the_provider_reports() {
        let mut first = FirstPaint {
            path: None,
            cache: Cache::default(),
            splashes: HashMap::new(),
            handled: HashSet::new(),
            keys: HashMap::new(),
            painted: HashSet::new(),
            laid_out: HashSet::new(),
            reported: HashSet::new(),
            started: Instant::now(),
        };
        first.cache.values.insert("tray".into(), "cached".into());
        assert_eq!(first.stand_in(Topic::Tray), Some("cached"));
        assert_eq!(first.stand_in(Topic::Clock), None, "only late topics");
        first.sent(Topic::Tray, "ignored before the report");
        first.reported(Topic::Tray);
        assert_eq!(first.stand_in(Topic::Tray), None);
        first.sent(Topic::Tray, "live");
        assert_eq!(first.cache.values["tray"], "live");
    }
}
