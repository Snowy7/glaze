//! The Windows taskbar's flat section layout, shared by live menus and visual fixtures.
use glaze_bars::jump_list::{Icon, Launch, Section};
use glaze_ui::{Entry, MenuIcon};
use std::sync::Arc;

/// Windows jump lists have no trailing chevron column and a consistent leading icon column.
pub fn style(tokens: &mut glaze_ui::Tokens) {
    tokens.menu.min_width = 280.0;
    tokens.menu.arrow = 0.0;
    tokens.menu.gap = 8.0;
    tokens.menu.pad = 8.0;
}

/// Turn worker pixels into the UI kit's existing icon type, with no I/O on the UI thread.
pub fn icon(icon: &Icon) -> MenuIcon {
    MenuIcon::Image(Arc::new(glaze_ui::Image {
        width: icon.width,
        height: icon.height,
        pixels: icon.pixels.clone(),
    }))
}

/// Section labels are non-interactive; destinations and Tasks stay directly beneath them.
pub fn rows<C>(sections: Vec<Section>, mut command: impl FnMut(Launch) -> C) -> Vec<Entry<C>> {
    let mut entries = Vec::new();
    for section in sections {
        if !section.items.iter().any(Option::is_some) {
            continue;
        }
        entries.push(Entry::Header(section.name));
        for item in section.items {
            entries.push(match item {
                Some(item) => {
                    let row = Entry::action(&item.name, command(item.launch));
                    match item.icon.as_deref() {
                        Some(image) => row.with_icon(icon(image)),
                        None => row,
                    }
                }
                None => Entry::Separator,
            });
        }
    }
    entries
}
