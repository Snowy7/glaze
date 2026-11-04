//! The main window, built in code so a developer can open it without taking
//! focus. Setting `MYPAPER_STUDIO_CAPTURE=x,y` (screen pixels) creates it
//! unfocused and always-on-top at that position (e.g. on a second monitor
//! while a game runs in front); without the variable it opens centered and
//! focused, as configured in `tauri.conf.json`.
//!
//! Capture mode (and its page-script hook) only exists in builds with the
//! `dev-capture` feature (`pnpm release:capture`); shipped builds ignore
//! both variables.

use tauri::{AppHandle, PhysicalPosition, WebviewWindow, WebviewWindowBuilder};

#[cfg(feature = "dev-capture")]
const ENV: &str = "MYPAPER_STUDIO_CAPTURE";
/// With capture mode: a JavaScript file to run in the page at startup.
const SCRIPT_ENV: &str = "MYPAPER_STUDIO_SCRIPT";

/// `"-2460, 109"` -> `(-2460, 109)`.
#[cfg(any(feature = "dev-capture", test))]
fn parse_position(text: &str) -> Option<(i32, i32)> {
    let (x, y) = text.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// Whether this is a capture session: a `dev-capture` build started with `MYPAPER_STUDIO_CAPTURE`.
pub fn active() -> bool {
    capture_position().is_some()
}

#[cfg(not(feature = "dev-capture"))]
fn capture_position() -> Option<(i32, i32)> {
    None
}

#[cfg(feature = "dev-capture")]
fn capture_position() -> Option<(i32, i32)> {
    let value = std::env::var(ENV).ok()?;
    let position = parse_position(&value);
    if position.is_none() {
        eprintln!("ignoring {ENV}={value:?}: expected \"x,y\" in screen pixels");
    }
    position
}

pub fn build_main_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let mut config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    let Some((x, y)) = capture_position() else {
        return WebviewWindowBuilder::from_config(app, &config)?.build();
    };
    config.center = false;
    // A window shown without activation lands behind the foreground one, so
    // keep it on top to be visible without ever taking focus. The builder
    // takes logical coordinates; the follow-up call pins the exact pixels.
    let mut builder = WebviewWindowBuilder::from_config(app, &config)?
        .focused(false)
        .always_on_top(true)
        .position(f64::from(x), f64::from(y));
    // Optionally run a script file inside the page, so a capture session can
    // walk through views on a timer without any OS input.
    if let Some(path) = std::env::var_os(SCRIPT_ENV) {
        match std::fs::read_to_string(&path) {
            Ok(js) => builder = builder.initialization_script(js),
            Err(e) => eprintln!("ignoring {SCRIPT_ENV}: {e}"),
        }
    }
    let window = builder.build()?;
    window.set_position(PhysicalPosition::new(x, y))?;
    Ok(window)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_are_two_comma_separated_pixel_values() {
        assert_eq!(parse_position("-2460,109"), Some((-2460, 109)));
        assert_eq!(parse_position(" 10 , -5 "), Some((10, -5)));
        assert_eq!(parse_position("10"), None);
        assert_eq!(parse_position("a,b"), None);
        assert_eq!(parse_position("1,2,3"), None);
    }
}
