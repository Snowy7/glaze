//! Recent wallpaper selections for the shared Home page.

use serde_json::Value;

use crate::app::App;
use crate::docs::DocId;

/// The active wallpaper, then the ones used recently (Studio remembers), then others.
pub(crate) fn recent(app: &App) -> Vec<usize> {
    let mut out: Vec<usize> = app.library.active().into_iter().collect();
    if let Some(list) = app
        .get(DocId::Studio, "/recent_wallpapers")
        .and_then(Value::as_array)
    {
        for name in list.iter().filter_map(Value::as_str) {
            if let Some(i) = app.library.items.iter().position(|w| w.apply_as == name)
                && !out.contains(&i)
            {
                out.push(i);
            }
        }
    }
    for (i, w) in app.library.items.iter().enumerate() {
        if out.len() >= 6 {
            break;
        }
        if !out.contains(&i) && w.error.is_none() {
            out.push(i);
        }
    }
    out
}
