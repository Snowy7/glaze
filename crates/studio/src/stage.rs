//! Picture helpers shared by Studio's pages: the display's shape for the wallpaper editor's
//! canvas, the shell's widget grid, moved display lists, and cover-cropped pictures. Also the
//! shell's app-icon service that the panel's app lists share.

use std::path::PathBuf;
use std::sync::Arc;

use glaze_ui::{Image, Op, Point, Rect};
use glaze_widgets::layout::{self, Grid, Layout, RectF};

use crate::app::App;
use crate::ui::B;

/// The shell's app-icon service (names and icons of pinned apps), started on first use.
#[derive(Default)]
pub struct Cache {
    icons: Option<(glaze_bars::icons::Icons, std::sync::mpsc::Receiver<glaze_bars::wake::Msg>)>,
}

impl Cache {
    /// Share the existing asynchronous shell icon worker with the panel's app lists.
    pub(crate) fn app_icon(&mut self, thread: u32, source: glaze_bars::icons::Source) -> Option<PathBuf> {
        // Studio only needs the picture here, not a shell item's display name or launch
        // metadata. A warm PNG must not load a native shell provider just to rediscover it.
        if let glaze_bars::icons::Source::Path(path) = &source {
            let cached = glaze_bars::files::local("icons").ok()?
                .join(glaze_bars::icons::cache_name(&path.to_lowercase()));
            if cached.is_file() { return Some(cached); }
        }
        if self.icons.is_none() && thread != 0 {
            let (tx, rx) = std::sync::mpsc::channel();
            let dir = glaze_bars::files::local("icons").ok()?;
            self.icons = Some((glaze_bars::icons::Icons::new(glaze_bars::wake::Waker::new(tx, thread), dir), rx));
        }
        self.icons.as_mut()?.0.get(&source)?.icon.as_ref().map(PathBuf::from)
    }

    /// Takes what the icon service found; returns whether pages showing app icons must redraw.
    pub fn icons_arrived(&mut self) -> bool {
        let Some((icons, rx)) = &mut self.icons else { return false };
        let mut any = false;
        while let Ok(msg) = rx.try_recv() {
            if let glaze_bars::wake::Msg::Described(source, described) = msg {
                icons.on_described(source, described);
                any = true;
            }
        }
        any
    }
}

/// Where the stage sits in `area`: the display's shape, as large as fits.
pub fn frame(app: &App, area: Rect) -> Rect {
    let (mw, mh) = app
        .display()
        .map_or((2560.0, 1440.0), |m| (m.rect.width as f32, m.rect.height as f32));
    let scale = (area.width() / mw).min(area.height() / mh);
    let (w, h) = ((mw * scale).floor(), (mh * scale).floor());
    Rect::new(
        (area.left + (area.width() - w) / 2.0).round(),
        (area.top + (area.height() - h) / 2.0).round(),
        w,
        h,
    )
}

/// A widget grid for a display, as the shell computes it.
pub fn grid_for(layout: &Layout, monitor: &glaze_desktop::Monitor) -> Grid {
    let scale = monitor.dpi as f64 / 96.0;
    Grid::new(
        &layout.grid,
        icon_cell(),
        RectF {
            x: f64::from(monitor.work.x - monitor.rect.x) / scale,
            y: f64::from(monitor.work.y - monitor.rect.y) / scale,
            width: f64::from(monitor.work.width) / scale,
            height: f64::from(monitor.work.height) / scale,
        },
    )
}

/// The desktop icon cell in DIPs (Windows' icon spacing), as the shell reads it.
fn icon_cell() -> [f64; 2] {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::w;
    let read = |name: windows::core::PCWSTR| {
        let mut buffer = [0u16; 32];
        let mut bytes = size_of_val(&buffer) as u32;
        // SAFETY: a bounded buffer and its size.
        unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!(r"Control Panel\Desktop\WindowMetrics"),
                name,
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
            .ok()
            .ok()?;
        }
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..len]).trim().parse::<i32>().ok()
    };
    [
        layout::icon_spacing_dip(read(w!("IconSpacing"))),
        layout::icon_spacing_dip(read(w!("IconVerticalSpacing"))),
    ]
}

/// `ops` moved by `(dx, dy)`, without text fields (native windows, not pixels).
/// stillkit's `Op::translate` moves each op, so a new op never needs a case here.
pub fn offset(ops: &[Op], dx: f32, dy: f32) -> impl Iterator<Item = Op> + '_ {
    ops.iter().filter_map(move |op| match op {
        Op::TextField { .. } => None,
        Op::Clipped { rect, ops } => Some(Op::Clipped {
            rect: rect.offset(dx, dy),
            ops: offset(ops, dx, dy).collect::<Vec<_>>().into(),
        }),
        Op::RoundedClipped { rect, radius, ops } => Some(Op::RoundedClipped {
            rect: rect.offset(dx, dy),
            radius: *radius,
            ops: offset(ops, dx, dy).collect::<Vec<_>>().into(),
        }),
        op => {
            let mut op = op.clone();
            op.translate(dx, dy);
            Some(op)
        }
    })
}

/// The centred crop of `image` that covers `rect` without stretching.
fn cover_source(rect: Rect, image: &Image) -> Rect {
    let (iw, ih) = (image.width as f32, image.height as f32);
    let scale = (rect.width() / iw).max(rect.height() / ih);
    let (sw, sh) = (rect.width() / scale, rect.height() / scale);
    Rect::new((iw - sw) / 2.0, (ih - sh) / 2.0, sw, sh)
}

/// `image` filling `rect`, cropped to its shape (cover).
pub fn image_cover(b: &mut B, rect: Rect, image: &Arc<Image>, opacity: f32) {
    b.list.ops.push(Op::Image {
        rect,
        image: image.clone(),
        opacity,
        source: Some(cover_source(rect, image)),
    });
}

/// [`image_cover`] with its corners rounded to `radius` (device px), clipped clean: pictures in
/// tiles and cards.
pub fn image_round(b: &mut B, rect: Rect, image: &Arc<Image>, opacity: f32, radius: f32) {
    b.list.ops.push(Op::RoundedImage {
        rect,
        image: image.clone(),
        opacity,
        source: Some(cover_source(rect, image)),
        radius,
    });
}

