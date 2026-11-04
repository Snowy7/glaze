//! The wallpaper snapshots the shell lenses under its widgets. File events drive every update.
use std::time::UNIX_EPOCH;

use mypaper_desktop::Monitor;
use serde_json::{Value, json};

// A separate host: WebView2 folder mappings bypass WebResourceRequested on shell.mypaper.
pub const PREFIX: &str = "https://backdrop.widgets.mypaper/";

pub fn request(active: bool) {
    let Ok(dir) = crate::files::local("backdrop") else {
        return;
    };
    let path = dir.join("widgets-request.json");
    if active {
        if !path.exists()
            && let Err(e) = crate::files::write_atomic(&path, b"{\"interval\":10}")
        {
            log::warn!("requesting widget backdrops: {e:#}");
        }
    } else {
        let _ = std::fs::remove_file(path);
    }
}

pub fn payload(monitor: &Monitor) -> Value {
    let name = monitor.device.trim_start_matches(r"\\.\");
    let Some(path) = crate::files::local("backdrop")
        .ok()
        .map(|d| d.join(format!("{name}.bmp")))
    else {
        return Value::Null;
    };
    let Ok(modified) = std::fs::metadata(path).and_then(|m| m.modified()) else {
        return Value::Null;
    };
    let version = modified
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let scale = f64::from(monitor.dpi) / 96.0;
    json!({ "url": format!("{PREFIX}{name}.bmp?v={version}"), "width": f64::from(monitor.rect.width) / scale, "height": f64::from(monitor.rect.height) / scale })
}

/// A shell-only URL, restricted to a single monitor BMP in the backdrop folder.
pub fn image(url: &str) -> Option<Vec<u8>> {
    std::fs::read(crate::files::local("backdrop").ok()?.join(image_name(url)?)).ok()
}

fn image_name(url: &str) -> Option<&str> {
    let name = url.strip_prefix(PREFIX)?.split('?').next()?;
    let digits = name.strip_prefix("DISPLAY")?.strip_suffix(".bmp")?;
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_monitor_images_are_served_by_the_shell_origin() {
        assert_eq!(
            image_name(&format!("{PREFIX}DISPLAY2.bmp?v=123")),
            Some("DISPLAY2.bmp")
        );
        for name in [
            "DISPLAY.bmp",
            "../DISPLAY2.bmp",
            "DISPLAY2.bmp/../private",
            "DISPLAY2.png",
            "DISPLAY2%2f.bmp",
        ] {
            assert!(image_name(&format!("{PREFIX}{name}")).is_none());
        }
        assert!(image_name("https://widget.mypaper/backdrop/DISPLAY2.bmp").is_none());
    }
}
