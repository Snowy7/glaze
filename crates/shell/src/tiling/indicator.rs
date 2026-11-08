//! Change-driven top-bar position track. No ticker or allocation before tiling is enabled.
use glaze_tiling::{Axis, Layout};
use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicU64, Ordering},
};
static REVISION: AtomicU64 = AtomicU64::new(0);
type Tracks = Vec<(String, f64, f64)>;
static TRACKS: OnceLock<Mutex<Tracks>> = OnceLock::new();
pub fn revision() -> u64 {
    REVISION.load(Ordering::Relaxed)
}
pub fn clear() {
    if let Some(tracks) = TRACKS.get() {
        tracks.lock().unwrap().clear();
        REVISION.fetch_add(1, Ordering::Relaxed);
    }
}
pub fn publish(layout: &Layout, displays: &[glaze_desktop::Monitor]) {
    let targets = layout.targets();
    let mut tracks = Vec::new();
    for m in &layout.monitors {
        let Some(display) = displays
            .iter()
            .find(|d| f64::from(d.rect.x) == m.full.x && f64::from(d.rect.y) == m.full.y)
        else {
            continue;
        };
        let (start, length) = match m.axis {
            Axis::Horizontal => (m.area.x, m.area.w),
            Axis::Vertical => (m.area.y, m.area.h),
        };
        let bounds = targets
            .iter()
            .filter(|t| t.workspace == m.active && !t.floating)
            .map(|t| match m.axis {
                Axis::Horizontal => (t.rect.x, t.rect.x + t.rect.w),
                Axis::Vertical => (t.rect.y, t.rect.y + t.rect.h),
            })
            .fold(None, |acc: Option<(f64, f64)>, (a, b)| {
                Some(acc.map_or((a, b), |(low, high)| (low.min(a), high.max(b))))
            });
        if let Some((low, high)) = bounds {
            let extent = (high - low).max(length);
            tracks.push((
                display.device.clone(),
                ((start - low) / extent).clamp(0.0, 1.0),
                (length / extent).min(1.0),
            ));
        }
    }
    let mut current = TRACKS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap();
    if *current != tracks {
        *current = tracks;
        REVISION.fetch_add(1, Ordering::Relaxed);
    }
}
/// Draw inside the bar's existing shape; no extra surface or reservation.
pub fn draw(frame: &mut crate::bars::view::Frame, device: &str, scale: f32) {
    let Some(tracks) = TRACKS.get() else {
        return;
    };
    let tracks = tracks.lock().unwrap();
    let Some((_, offset, length)) = tracks.iter().find(|(id, _, _)| id == device) else {
        return;
    };
    let Some((body, _)) = frame.shapes.first() else {
        return;
    };
    let width = (72.0 * scale).min(body.width() * 0.3);
    let x = body.left + (body.width() - width) / 2.0;
    let y = body.bottom - 3.0 * scale;
    use glaze_ui::{Op, Rect, Rgba};
    frame.list.ops.push(Op::Fill {
        rect: Rect::new(x, y, width, 2.0 * scale),
        radius: scale,
        color: Rgba::rgb8([145, 155, 166], 0.25),
    });
    frame.list.ops.push(Op::Fill {
        rect: Rect::new(
            x + width * (*offset as f32),
            y,
            (width * (*length as f32)).max(3.0 * scale),
            2.0 * scale,
        ),
        radius: scale,
        color: Rgba::rgb8([132, 197, 242], 0.9),
    });
}
