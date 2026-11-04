//! The overlay layer (docs/widgets.md, "Placement presets"): which layer shows an instance,
//! and the topmost window that holds the overlay widgets of one monitor.
//!
//! The overlay window is a [`Surface`](crate::surface::Surface) like the desktop layer's, with
//! the same shell page and WebView; it differs in being a top-level `WS_EX_TOPMOST` tool
//! window whose region is just the overlay widgets, so everything else clicks through. The
//! rules for what goes where are pure and live here.

use anyhow::Result;
use mypaper_desktop::{Monitor, Rect};
use serde::Serialize;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos, WS_CLIPCHILDREN,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::manifest::Preset;

/// The registered window message that turns the overlay on, off or over (`wParam` 1, 0, 2).
pub const OVERLAY_MESSAGE: PCWSTR = w!("mypaper.widgets.overlay");
/// Ctrl+Alt+Shift+O.
pub const OVERLAY_KEY: u32 = b'O' as u32;

/// Which window of a monitor shows an instance. Serialized as the state's `layer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    /// The desktop layer, above the icons and below every app.
    Desktop,
    /// The topmost layer.
    Overlay,
}

/// Where an instance of a widget with `preset` is shown right now, if anywhere. Popups are
/// the bars host's; overlay widgets are edited in the desktop layer (the overlay layer is
/// hidden in edit mode, so each instance only runs once), and are shown nowhere while the
/// overlay is switched off.
pub fn layer_for(preset: Preset, editing: bool, overlay_on: bool) -> Option<Layer> {
    match preset {
        Preset::Popup => None,
        Preset::Desktop => Some(Layer::Desktop),
        Preset::Overlay if editing => Some(Layer::Desktop),
        Preset::Overlay => overlay_on.then_some(Layer::Overlay),
    }
}

/// Whether `layer` needs a window on a monitor where `has_instances` says some instance
/// shows in it. The desktop layer also exists on every monitor while editing, so widgets can
/// be added anywhere; the overlay layer never does.
pub fn layer_wanted(layer: Layer, editing: bool, overlay_on: bool, has_instances: bool) -> bool {
    match layer {
        Layer::Desktop => editing || has_instances,
        Layer::Overlay => overlay_on && !editing && has_instances,
    }
}

/// What a `wParam` of the edit and overlay messages asks for, given the current state:
/// 0 off, 1 on, 2 toggle.
pub fn requested(current: bool, wparam: usize) -> Option<bool> {
    match wparam {
        0 => Some(false),
        1 => Some(true),
        2 => Some(!current),
        _ => None,
    }
}

/// Creates the (empty, hidden-until-placed) topmost window for `monitor`. `class` is the
/// surface window class.
pub fn create_window(class: PCWSTR, monitor: &Monitor) -> Result<HWND> {
    // SAFETY: the class is registered by `surface::register_class`; a top-level window has no
    // parent.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!("mypaper overlay"),
            WS_POPUP | WS_CLIPCHILDREN,
            monitor.rect.x,
            monitor.rect.y,
            monitor.rect.width,
            monitor.rect.height,
            None,
            None,
            None,
            None,
        )?
    };
    Ok(hwnd)
}

/// Moves the window over `rect` (screen pixels), topmost, without activating it.
pub fn place(hwnd: HWND, rect: Rect) -> Result<()> {
    // SAFETY: our window.
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_pick_their_layer() {
        use Preset::{Desktop, Overlay, Popup};
        // (preset, editing, overlay on) -> layer
        for (preset, editing, on, layer) in [
            (Desktop, false, true, Some(Layer::Desktop)),
            (Desktop, true, false, Some(Layer::Desktop)),
            (Overlay, false, true, Some(Layer::Overlay)),
            (Overlay, false, false, None),
            (Overlay, true, true, Some(Layer::Desktop)),
            (Overlay, true, false, Some(Layer::Desktop)),
            (Popup, false, true, None),
            (Popup, true, true, None),
        ] {
            assert_eq!(
                layer_for(preset, editing, on),
                layer,
                "{preset:?} editing={editing} on={on}"
            );
        }
    }

    #[test]
    fn layers_exist_only_where_they_have_something_to_show() {
        // Desktop: instances, or edit mode everywhere.
        assert!(!layer_wanted(Layer::Desktop, false, true, false));
        assert!(layer_wanted(Layer::Desktop, false, true, true));
        assert!(layer_wanted(Layer::Desktop, true, true, false));
        // Overlay: only with instances, switched on, and not editing.
        assert!(layer_wanted(Layer::Overlay, false, true, true));
        assert!(!layer_wanted(Layer::Overlay, false, true, false));
        assert!(!layer_wanted(Layer::Overlay, false, false, true));
        assert!(!layer_wanted(Layer::Overlay, true, true, true));
    }

    #[test]
    fn the_message_asks_for_on_off_or_toggle() {
        assert_eq!(requested(true, 0), Some(false));
        assert_eq!(requested(false, 1), Some(true));
        assert_eq!(requested(true, 2), Some(false));
        assert_eq!(requested(false, 2), Some(true));
        assert_eq!(requested(false, 7), None);
    }

    #[test]
    fn layers_serialize_as_the_state_says() {
        assert_eq!(serde_json::to_value(Layer::Desktop).unwrap(), "desktop");
        assert_eq!(serde_json::to_value(Layer::Overlay).unwrap(), "overlay");
    }
}
