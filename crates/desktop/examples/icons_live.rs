//! Shows where the desktop icons are (temporarily showing them if they are hidden, then hiding
//! them again): the rectangles the right-click hook consults, per monitor.
//!
//! ```text
//! cargo run -p glaze-desktop --example icons_live
//! ```

use std::time::{Duration, Instant};

use anyhow::Result;
use glaze_desktop::desktop_view::DesktopView;

fn main() -> Result<()> {
    glaze_desktop::enable_dpi_awareness();
    let view = DesktopView::open()?;
    let was_visible = view.icons_visible()?;
    if !was_visible {
        view.set_icons_visible(true)?;
    }
    let started = Instant::now();
    let mut rects = Vec::new();
    while started.elapsed() < Duration::from_secs(3) {
        rects = view.icon_rects()?;
        if !rects.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    println!("{} icons after {:.0?}", rects.len(), started.elapsed());
    let monitors = glaze_desktop::monitors();
    for (i, monitor) in monitors.iter().enumerate() {
        let inside: Vec<_> = rects
            .iter()
            .filter(|r| monitor.contains_point(r.x + 1, r.y + 1))
            .collect();
        println!("monitor {i} {monitor:?}: {} icons", inside.len());
        for rect in inside.iter().take(3) {
            println!("    {rect:?}");
        }
    }
    // Cross-check against what UI Automation says the desktop's list items occupy: every item's
    // box should lie in (and its center inside) the cell the folder view gave us for it.
    let checked = cross_check(&view, &rects);
    if !was_visible {
        view.set_icons_visible(false)?;
        println!("icons hidden again: {}", !view.icons_visible()?);
    }
    checked
}

fn cross_check(view: &DesktopView, rects: &[glaze_desktop::Rect]) -> Result<()> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation, TreeScope_Descendants};
    use windows::Win32::UI::WindowsAndMessaging::FindWindowExW;
    use windows::core::{PCWSTR, w};

    // SAFETY: documented COM and window calls on interfaces we own, in the apartment `open` set up.
    unsafe {
        let list = FindWindowExW(
            Some(view.window()?),
            None,
            w!("SysListView32"),
            PCWSTR::null(),
        )?;
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        let items = uia
            .ElementFromHandle(list)?
            .FindAll(TreeScope_Descendants, &uia.CreateTrueCondition()?)?;
        let count = items.Length()?;
        let kinds: std::collections::BTreeSet<i32> = (0..count)
            .filter_map(|i| items.GetElement(i).ok()?.CurrentControlType().ok())
            .map(|kind| kind.0)
            .collect();
        println!(
            "control types among the items: {kinds:?} (ListItem is {}, List is {})",
            windows::Win32::UI::Accessibility::UIA_ListItemControlTypeId.0,
            windows::Win32::UI::Accessibility::UIA_ListControlTypeId.0
        );
        let list_kind = uia.ElementFromHandle(list)?.CurrentControlType()?.0;
        println!("the desktop list itself (what bare space reports): {list_kind}");
        let (mut inside, mut total) = (0, 0);
        let mut worst = 0;
        for i in 0..count {
            let bounds = items.GetElement(i)?.CurrentBoundingRectangle()?;
            let (cx, cy) = (
                (bounds.left + bounds.right) / 2,
                (bounds.top + bounds.bottom) / 2,
            );
            if bounds.right <= bounds.left {
                continue; // an offscreen or empty item
            }
            total += 1;
            if rects.iter().any(|r| r.contains_point(cx, cy)) {
                inside += 1;
            } else {
                worst += 1;
            }
        }
        println!(
            "UI Automation lists {count} desktop items; {inside} of {total} sized ones have their center inside a cached icon cell ({worst} outside)"
        );
    }
    Ok(())
}
