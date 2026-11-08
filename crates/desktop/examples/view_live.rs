//! Exercises the desktop folder-view operations against the REAL desktop and puts everything
//! back: the icon size, the sort order, whether the icons are shown, and the item rectangles.
//!
//! ```text
//! cargo run -p glaze-desktop --example view_live
//! ```
//!
//! A new item is NOT created (that would leave a file behind); `DesktopView::create` is covered by
//! the menu harness when asked for.

use anyhow::{Result, bail};
use glaze_desktop::desktop_view::{DesktopView, IconSize, SortKey};

fn main() -> Result<()> {
    let view = DesktopView::open()?;
    let size = view.icon_size()?;
    let sort = view.sort()?;
    let visible = view.icons_visible()?;
    println!("before: icon size {size:?}, sort {sort:?}, icons visible {visible}");
    println!("desktop window: {:?}", view.window()?);
    println!("icons: {} rectangles", view.icon_rects()?.len());

    // Icon size: each preset, read back, then the original.
    for target in [IconSize::Large, IconSize::Medium, IconSize::Small] {
        view.set_icon_size(target)?;
        let now = view.icon_size()?;
        println!("set {target:?} -> reads {now:?}");
        if now != target {
            bail!("icon size did not stick");
        }
    }
    view.set_icon_size(size)?;
    println!("icon size restored: {}", view.icon_size()? == size);

    // Show / hide the icons, and the rectangles that come with them.
    view.set_icons_visible(!visible)?;
    std::thread::sleep(std::time::Duration::from_millis(300));
    let toggled = view.icons_visible()?;
    println!(
        "icons visible after toggling: {toggled} (was {visible}); rectangles: {}",
        view.icon_rects()?.len()
    );
    view.set_icons_visible(visible)?;
    std::thread::sleep(std::time::Duration::from_millis(300));
    println!(
        "icons visibility restored: {}",
        view.icons_visible()? == visible
    );

    // Sort: only exercised when the view reports a sort we can put back exactly.
    match sort {
        Some((key, ascending)) => {
            let other = SortKey::ALL
                .into_iter()
                .find(|k| *k != key)
                .expect("four keys");
            view.set_sort(other, !ascending)?;
            println!(
                "set {other:?} descending/ascending flipped -> reads {:?}",
                view.sort()?
            );
            view.set_sort(key, ascending)?;
            println!("sort restored: {}", view.sort()? == sort);
        }
        None => println!("the desktop reports no sort column; not changing it"),
    }
    view.refresh()?;
    println!("refreshed");
    Ok(())
}
