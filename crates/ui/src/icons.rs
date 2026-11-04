//! Icon fonts: an icon is a glyph of an icon font, drawn as text. Segoe Fluent Icons ships with
//! Windows 11; Windows 10 has the same code points in Segoe MDL2 Assets. Lucide and Fluent UI
//! System Icons (bundled TTFs) come with the styles (N6).

use std::sync::Arc;

use crate::text::Text;

pub const SYSTEM_FAMILY: &str = "Segoe Fluent Icons";
const SYSTEM_FALLBACK: &str = "Segoe MDL2 Assets";

/// Segoe Fluent Icons / MDL2 code points.
pub const CHECK: char = '\u{E73E}';
pub const CHEVRON_RIGHT: char = '\u{E76C}';

/// The system icon font that is installed here.
pub fn system_family(text: &Text) -> Arc<str> {
    if text.has_family(SYSTEM_FAMILY) {
        SYSTEM_FAMILY.into()
    } else {
        SYSTEM_FALLBACK.into()
    }
}
