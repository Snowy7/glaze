//! WebView2 as the widget host and the bars use it (feature `webview`): how their environment is
//! created, and the first-paint probe the perf markers and the bars' native first paint rely on
//! (docs/performance.md).
//!
//! WebView2 starts one browser process per user data folder *and* set of environment options.
//! Every host that calls [`create_environment`] with the same folder gets the same options, so
//! they can share a browser process.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use webview2_com::CoreWebView2EnvironmentOptions;
use webview2_com::CreateCoreWebView2EnvironmentCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2Environment,
    ICoreWebView2EnvironmentOptions,
};
use windows::Win32::Foundation::E_POINTER;
use windows::core::{HSTRING, PCWSTR};

/// Run in a page once it has its first data: it answers [`PAINTED`] after the next frame was
/// produced (two animation frames), i.e. once that data is on screen.
pub const PAINT_PROBE: &str = "requestAnimationFrame(() => requestAnimationFrame(() => \
     window.chrome.webview.postMessage('mypaper:painted')))";

/// The probe's answer, as `WebMessageAsJson` reports it.
pub const PAINTED: &str = "\"mypaper:painted\"";

/// `%LOCALAPPDATA%\mypaper\webview2-shell`: the profile the widget host and the bars share, so
/// they share one browser process (and its GPU and utility processes). Their pages have
/// different origins, so their storage stays apart.
pub fn shared_data_dir() -> Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(local).join("mypaper").join("webview2-shell"))
}

/// Starts creating an environment on `data_dir` (created if missing) with the options every
/// mypaper host uses. `done` runs on this thread, from inside message dispatch.
pub fn create_environment(
    data_dir: &Path,
    done: impl FnOnce(windows::core::Result<ICoreWebView2Environment>) + 'static,
) -> Result<()> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("creating {}", data_dir.display()))?;
    let handler =
        CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |result, env| {
            done(result.and_then(|()| env.ok_or_else(|| E_POINTER.into())));
            Ok(())
        }));
    let options: ICoreWebView2EnvironmentOptions = CoreWebView2EnvironmentOptions::default().into();
    // SAFETY: valid strings and COM objects that outlive the call.
    unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            &HSTRING::from(plain(data_dir)),
            &options,
            &handler,
        )
    }
    .context("starting WebView2 (is the WebView2 Runtime installed?)")
}

/// A path for WebView2 APIs: absolute, without the `\\?\` prefix.
fn plain(path: &Path) -> String {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\")
        .map(str::to_owned)
        .unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_paths_drop_the_verbatim_prefix() {
        assert_eq!(plain(Path::new(r"\\?\C:\x\y")), r"C:\x\y");
        assert_eq!(plain(Path::new(r"C:\x")), r"C:\x");
    }

    #[test]
    fn the_probe_answers_what_hosts_look_for() {
        let answer = PAINT_PROBE.split('\'').nth(1).unwrap();
        assert_eq!(PAINTED, format!("\"{answer}\""));
    }
}
