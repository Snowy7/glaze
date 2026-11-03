//! `web` wallpapers: a WebView2 per screen, as a child of the screen's window.
//!
//! WebView2 is asynchronous: the environment and every controller arrive
//! through completion handlers, which the engine's message loop dispatches on
//! the main thread (a single-threaded COM apartment, see
//! [`init_com_apartment`](crate::window::init_com_apartment)). Handlers hold
//! only a `Weak` reference to the shared state, so a source dropped (theme
//! switch) before they fire is simply ignored.
//!
//! Lightness: a hidden screen's WebView is hidden and then suspended
//! (`TrySuspend`), which stops its scripts, timers and rendering, and lets
//! the browser trim its memory. The page resumes where it left off.
//!
//! A paused screen can still be *seen* (the second monitor while a game is
//! fullscreen), and hiding the WebView would reveal whatever the screen's
//! swapchain last showed, often a previous theme. So pausing first captures
//! a still of the page (`CapturePreview`), hands the screen to the swapchain
//! to present it, and only then hides and suspends the WebView.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use mypaper_theme::{Fit, LoadedTheme, WebWallpaper};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, COREWEBVIEW2_COLOR,
    COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY_CORS, CreateCoreWebView2EnvironmentWithOptions,
    ICoreWebView2, ICoreWebView2_3, ICoreWebView2Controller, ICoreWebView2Controller2,
    ICoreWebView2Environment, ICoreWebView2EnvironmentOptions, ICoreWebView2Settings3,
    ICoreWebView2Settings5, ICoreWebView2Settings6,
};
use webview2_com::{
    CapturePreviewCompletedHandler, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    NavigationCompletedEventHandler, TrySuspendCompletedHandler,
};
use windows::Win32::Foundation::{HGLOBAL, HWND, RECT};
use windows::Win32::System::Com::IStream;
use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::core::{HSTRING, Interface, PCWSTR};

use super::wic::{Budget, Decoded, Decoder, Picture};
use super::{Frame, Redraw, Source, Visibility, begin};
use crate::gpu::Gpu;

/// Name under which a local page's folder is served (`https://mypaper.theme/...`).
/// Unlike `file://`, a virtual host gives the page a real origin, so `fetch`,
/// ES modules and workers work with relative paths.
const HOST: &str = "mypaper.theme";

/// Browser flags: no SmartScreen lookups (a wallpaper loads its own content),
/// and autoplay without a user gesture (nobody clicks a wallpaper). Background
/// throttling stays on, so hidden frames and timers are throttled by the browser.
const BROWSER_ARGS: &str =
    "--disable-features=msSmartScreenProtection --autoplay-policy=no-user-gesture-required";

/// Shown before the first paint, so a loading page is black rather than white.
const BACKGROUND: COREWEBVIEW2_COLOR = COREWEBVIEW2_COLOR {
    A: 255,
    R: 0,
    G: 0,
    B: 0,
};

/// What a theme's `url` points at.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Page {
    /// An `http(s)` URL, navigated to as written.
    Remote(String),
    /// A local file, served from `folder` under [`HOST`] at `url`.
    Local { folder: PathBuf, url: String },
}

impl Page {
    /// Resolves a theme's `url`. `file` is the local path it would be
    /// (`LoadedTheme::asset`), used when `url` is not remote. A file inside the
    /// theme folder serves the whole theme folder (so `../`-free sibling assets
    /// and subfolders work); one outside it serves just its own folder.
    fn resolve(url: &str, file: &Path, theme_dir: &Path) -> Self {
        if is_remote(url) {
            return Self::Remote(url.to_owned());
        }
        let (folder, relative) = match file.strip_prefix(theme_dir) {
            Ok(relative) => (theme_dir, relative),
            Err(_) => (
                file.parent().unwrap_or(file),
                Path::new(file.file_name().unwrap_or_default()),
            ),
        };
        let path = relative
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(name) => Some(percent_encode(&name.to_string_lossy())),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/");
        Self::Local {
            folder: folder.to_owned(),
            url: format!("https://{HOST}/{path}"),
        }
    }
}

fn is_remote(url: &str) -> bool {
    let scheme = |s: &str| {
        url.get(..s.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(s))
    };
    scheme("https://") || scheme("http://")
}

/// Percent-encodes everything but unreserved characters, for one URL path segment.
fn percent_encode(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `%LOCALAPPDATA%\mypaper\webview2`: one profile shared by every web theme.
fn user_data_folder() -> Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    let dir = PathBuf::from(local).join("mypaper").join("webview2");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

/// A page on every screen. Dropping it closes the WebViews and releases the
/// environment, which lets the browser processes exit.
pub struct WebSource {
    state: Rc<RefCell<State>>,
}

struct State {
    page: Page,
    /// Set once the environment exists; kept so it outlives the controllers.
    environment: Option<ICoreWebView2Environment>,
    /// The source was dropped: late completions must clean up and do nothing.
    closed: bool,
    screens: Vec<ScreenView>,
}

struct ScreenView {
    hwnd: HWND,
    /// What the engine last said. Applied as soon as the view exists.
    visibility: Visibility,
    view: Option<View>,
    still: Still,
}

/// What a paused screen shows instead of the live page.
enum Still {
    /// Live: the WebView is on screen.
    None,
    /// Capturing a still; the WebView stays visible until it arrives.
    Capturing,
    /// The swapchain shows `image` (black if `None`). `fresh` until the engine
    /// has been told to present it; `hide` until that present happened.
    Ready {
        image: Option<Decoded>,
        picture: Option<Picture>,
        fresh: bool,
        hide: bool,
    },
}

impl Still {
    fn ready(image: Option<Decoded>) -> Self {
        Self::Ready {
            image,
            picture: None,
            fresh: true,
            hide: true,
        }
    }
}

/// Cheap to clone (COM reference counts): callers copy a view out of the
/// shared state before calling into WebView2, so no `RefCell` borrow is held
/// while WebView2 might call back.
#[derive(Clone)]
struct View {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2_3,
    /// The first navigation finished, so the page can be suspended.
    loaded: bool,
}

impl WebSource {
    /// Starts creating the environment and a WebView per screen. Fails
    /// at once if WebView2 is missing or the page is a local file that does
    /// not exist; later failures are logged, since they arrive asynchronously.
    pub fn new(theme: &LoadedTheme, config: &WebWallpaper, screens: &[HWND]) -> Result<Self> {
        let page = {
            let file = theme.asset(Path::new(&config.url));
            let page = Page::resolve(&config.url, &file, &theme.dir);
            if matches!(page, Page::Local { .. }) && !file.is_file() {
                bail!("web page {} does not exist", file.display());
            }
            page
        };
        let state = Rc::new(RefCell::new(State {
            page,
            environment: None,
            closed: false,
            screens: screens
                .iter()
                .map(|&hwnd| ScreenView {
                    hwnd,
                    visibility: Visibility::Live,
                    view: None,
                    still: Still::None,
                })
                .collect(),
        }));

        let options = CoreWebView2EnvironmentOptions::default();
        // SAFETY: `options` is not shared with WebView2 yet.
        unsafe { options.set_additional_browser_arguments(BROWSER_ARGS.into()) };
        let options: ICoreWebView2EnvironmentOptions = options.into();
        let user_data = HSTRING::from(user_data_folder()?.to_string_lossy().as_ref());

        let weak = Rc::downgrade(&state);
        let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
            move |result, environment| {
                match (result, environment) {
                    (Ok(()), Some(environment)) => environment_ready(&weak, environment),
                    (Err(e), _) => log::error!("WebView2 environment failed: {e}"),
                    (Ok(()), None) => log::error!("WebView2 environment failed: none returned"),
                }
                Ok(())
            },
        ));
        // SAFETY: valid arguments; the handler runs later on this thread's message loop.
        unsafe {
            CreateCoreWebView2EnvironmentWithOptions(PCWSTR::null(), &user_data, &options, &handler)
        }
        .context(
            "creating the WebView2 environment (is the WebView2 Runtime installed? \
             https://developer.microsoft.com/microsoft-edge/webview2/)",
        )?;
        Ok(Self { state })
    }
}

impl Source for WebSource {
    /// Only reached for a paused screen showing its still (see [`Still`]).
    fn render(&mut self, gpu: &Gpu, frame: Frame<'_>) {
        let mut state = self.state.borrow_mut();
        let Some(Still::Ready { image, picture, .. }) =
            state.screens.get_mut(frame.screen).map(|s| &mut s.still)
        else {
            return;
        };
        if picture.is_none()
            && let Some(image) = image.take()
        {
            match Picture::upload(gpu, "web still", &image) {
                Ok(uploaded) => *picture = Some(uploaded),
                Err(e) => log::warn!("web: still frame: {e:#}"),
            }
        }
        let mut pass = begin(
            frame.encoder,
            frame.view,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        if let Some(picture) = picture {
            picture.draw(gpu, &mut pass, frame.size, Fit::Cover, 1.0);
        }
    }

    fn redraw(&self, _clock: Duration) -> Redraw {
        Redraw::OnChange
    }

    fn native(&self, screen: usize) -> bool {
        let state = self.state.borrow();
        !matches!(
            state.screens.get(screen).map(|s| &s.still),
            Some(Still::Ready { .. })
        )
    }

    fn take_dirty(&mut self, screen: usize) -> bool {
        let mut state = self.state.borrow_mut();
        match state.screens.get_mut(screen).map(|s| &mut s.still) {
            Some(Still::Ready { fresh, .. }) => std::mem::take(fresh),
            _ => false,
        }
    }

    fn presented(&mut self, screen: usize) {
        let view = {
            let mut state = self.state.borrow_mut();
            let Some(slot) = state.screens.get_mut(screen) else {
                return;
            };
            let hide = match &mut slot.still {
                Still::Ready { hide, .. } => std::mem::take(hide),
                _ => false,
            };
            hide.then(|| slot.view.clone()).flatten()
        };
        // The still is on screen now, so the page can go without a flash.
        if let Some(view) = view {
            view.set_visible(false);
        }
    }

    fn layout(&mut self, screen: usize, hwnd: HWND, _size: (u32, u32)) {
        let view = (self.state.borrow().screens.get(screen)).and_then(|s| s.view.clone());
        if let Some(view) = view {
            fill(&view.controller, hwnd);
        }
    }

    fn set_visibility(&mut self, screen: usize, _hwnd: HWND, visibility: Visibility) {
        let (view, previous, has_still) = {
            let mut state = self.state.borrow_mut();
            let Some(slot) = state.screens.get_mut(screen) else {
                return;
            };
            let previous = std::mem::replace(&mut slot.visibility, visibility);
            match visibility {
                Visibility::Live => slot.still = Still::None,
                // Nobody would see a still that is still being captured.
                Visibility::Hidden if matches!(slot.still, Still::Capturing) => {
                    slot.still = Still::None;
                }
                _ => {}
            }
            let has_still = matches!(slot.still, Still::Ready { .. });
            (slot.view.clone(), previous, has_still)
        };
        match (visibility, view) {
            (Visibility::Live, Some(view)) => view.set_visible(true),
            (Visibility::Hidden, Some(view)) => view.set_visible(false),
            // A still from an earlier pause: the engine presents it again.
            (Visibility::Still, _) if has_still => {}
            (Visibility::Still, Some(view)) => {
                if previous == Visibility::Hidden {
                    view.set_visible(true); // a hidden page renders nothing to capture
                }
                // Loaded: capture now. Still loading: `navigation_done` does it.
                if view.loaded {
                    freeze(&Rc::downgrade(&self.state), screen, &view);
                }
            }
            // No WebView yet: nothing to capture; show black, not a stale frame.
            (Visibility::Still, None) => {
                self.state.borrow_mut().screens[screen].still = Still::ready(None);
            }
            (Visibility::Live | Visibility::Hidden, None) => {}
        }
    }
}

impl Drop for WebSource {
    fn drop(&mut self) {
        let (views, environment) = {
            let mut state = self.state.borrow_mut();
            state.closed = true;
            let views: Vec<View> = state
                .screens
                .iter_mut()
                .filter_map(|s| s.view.take())
                .collect();
            (views, state.environment.take())
        };
        for view in views {
            // SAFETY: closing a controller we own; its parent window still exists.
            let _ = unsafe { view.controller.Close() };
        }
        drop(environment);
    }
}

impl View {
    /// Hides and suspends the page, or resumes and shows it. Suspending is
    /// deferred until the first navigation finishes (see [`navigation_done`]).
    fn set_visible(&self, visible: bool) {
        // SAFETY: calls on COM objects we own, from their creating thread.
        unsafe {
            if visible {
                // Resume before showing; both fail harmlessly if nothing was suspended.
                let _ = self.webview.Resume();
                let _ = self.controller.SetIsVisible(true);
            } else {
                let _ = self.controller.SetIsVisible(false);
                if self.loaded {
                    self.suspend();
                }
            }
        }
    }

    /// Asks the browser to suspend the (already hidden) page.
    ///
    /// # Safety
    /// Must be called on the thread that created the WebView.
    unsafe fn suspend(&self) {
        let done = TrySuspendCompletedHandler::create(Box::new(|result, suspended| {
            log::debug!("web: suspend {result:?}, suspended={suspended}");
            Ok(())
        }));
        // SAFETY: per this function's contract.
        if let Err(e) = unsafe { self.webview.TrySuspend(&done) } {
            log::debug!("web: TrySuspend: {e}");
        }
    }
}

/// The environment exists: create a controller for every screen.
fn environment_ready(weak: &Weak<RefCell<State>>, environment: ICoreWebView2Environment) {
    let Some(state) = weak.upgrade() else { return };
    let hwnds: Vec<HWND> = {
        let mut state = state.borrow_mut();
        if state.closed {
            return;
        }
        state.environment = Some(environment.clone());
        state.screens.iter().map(|s| s.hwnd).collect()
    };
    for (screen, hwnd) in hwnds.into_iter().enumerate() {
        let weak = weak.clone();
        let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
            move |result, controller| {
                match (result, controller) {
                    (Ok(()), Some(controller)) => controller_ready(&weak, screen, controller),
                    (Err(e), _) => log::error!("WebView2 controller {screen} failed: {e}"),
                    (Ok(()), None) => log::error!("WebView2 controller {screen}: none returned"),
                }
                Ok(())
            },
        ));
        // SAFETY: `hwnd` is a live screen window of this thread.
        if let Err(e) = unsafe { environment.CreateCoreWebView2Controller(hwnd, &handler) } {
            log::error!("WebView2 controller {screen} failed: {e}");
        }
    }
}

/// A controller exists: configure it, load the page and apply the screen's visibility.
fn controller_ready(
    weak: &Weak<RefCell<State>>,
    screen: usize,
    controller: ICoreWebView2Controller,
) {
    let Some(state) = weak.upgrade() else {
        // SAFETY: closing the controller we were just handed.
        let _ = unsafe { controller.Close() };
        return;
    };
    let (hwnd, visibility, page) = {
        let state = state.borrow();
        if state.closed {
            // SAFETY: as above.
            let _ = unsafe { controller.Close() };
            return;
        }
        let slot = &state.screens[screen];
        (slot.hwnd, slot.visibility, state.page.clone())
    };
    match setup(weak, screen, controller.clone(), hwnd, &page) {
        Ok(view) => {
            // A paused but seen screen keeps its page visible while it loads:
            // `navigation_done` captures a still, then hides it.
            view.set_visible(visibility != Visibility::Hidden);
            state.borrow_mut().screens[screen].view = Some(view);
        }
        Err(e) => {
            log::error!("web: setting up screen {screen}: {e:#}");
            // SAFETY: closing the controller we own.
            let _ = unsafe { controller.Close() };
        }
    }
}

/// Fills the screen, applies the wallpaper-appropriate settings and navigates.
fn setup(
    weak: &Weak<RefCell<State>>,
    screen: usize,
    controller: ICoreWebView2Controller,
    hwnd: HWND,
    page: &Page,
) -> Result<View> {
    // SAFETY: COM calls on objects owned by this thread.
    unsafe {
        fill(&controller, hwnd);
        if let Ok(controller2) = controller.cast::<ICoreWebView2Controller2>() {
            controller2.SetDefaultBackgroundColor(BACKGROUND)?;
        }
        let webview: ICoreWebView2 = controller.CoreWebView2()?;

        // A wallpaper is not a browser: no menus, tools, zoom, dialogs or shortcuts.
        let settings = webview.Settings()?;
        settings.SetAreDefaultContextMenusEnabled(false)?;
        settings.SetAreDevToolsEnabled(false)?;
        settings.SetIsStatusBarEnabled(false)?;
        settings.SetIsZoomControlEnabled(false)?;
        settings.SetAreDefaultScriptDialogsEnabled(false)?;
        // Newer settings are best-effort: older runtimes lack the interfaces.
        if let Ok(s) = settings.cast::<ICoreWebView2Settings3>() {
            let _ = s.SetAreBrowserAcceleratorKeysEnabled(false);
        }
        if let Ok(s) = settings.cast::<ICoreWebView2Settings5>() {
            let _ = s.SetIsPinchZoomEnabled(false);
        }
        if let Ok(s) = settings.cast::<ICoreWebView2Settings6>() {
            let _ = s.SetIsSwipeNavigationEnabled(false);
        }

        let webview3 = webview
            .cast::<ICoreWebView2_3>()
            .context("the installed WebView2 Runtime is too old (needs ICoreWebView2_3)")?;
        let url = match page {
            Page::Remote(url) => url,
            Page::Local { folder, url } => {
                webview3.SetVirtualHostNameToFolderMapping(
                    &HSTRING::from(HOST),
                    &HSTRING::from(folder.to_string_lossy().as_ref()),
                    COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY_CORS,
                )?;
                url
            }
        };

        let weak = weak.clone();
        let handler = NavigationCompletedEventHandler::create(Box::new(move |_, _| {
            navigation_done(&weak, screen);
            Ok(())
        }));
        let mut token = 0;
        webview.add_NavigationCompleted(&handler, &mut token)?;
        webview
            .Navigate(&HSTRING::from(url.as_str()))
            .with_context(|| format!("navigating to {url}"))?;

        Ok(View {
            controller,
            webview: webview3,
            loaded: false,
        })
    }
}

/// The page finished loading. A screen paused in the meantime can only be
/// captured (if seen) or suspended (if hidden) now.
fn navigation_done(weak: &Weak<RefCell<State>>, screen: usize) {
    let Some(state) = weak.upgrade() else { return };
    let first_load = {
        let mut state = state.borrow_mut();
        let slot = &mut state.screens[screen];
        let visibility = slot.visibility;
        slot.view.as_mut().and_then(|view| {
            let first = !std::mem::replace(&mut view.loaded, true);
            first.then(|| (view.clone(), visibility))
        })
    };
    match first_load {
        Some((view, Visibility::Still)) => freeze(weak, screen, &view),
        // SAFETY: handlers run on the thread that created the WebView.
        Some((view, Visibility::Hidden)) => unsafe { view.suspend() },
        Some((_, Visibility::Live)) | None => {}
    }
}

/// Starts capturing a still of a paused screen's page. The WebView stays on
/// screen until the still is presented in its place (see [`Still`]).
fn freeze(weak: &Weak<RefCell<State>>, screen: usize, view: &View) {
    let Some(state) = weak.upgrade() else { return };
    state.borrow_mut().screens[screen].still = Still::Capturing;
    // SAFETY: creates a growable in-memory stream that owns its memory.
    let stream = match unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) } {
        Ok(stream) => stream,
        Err(e) => return captured(weak, screen, Err(e.into())),
    };
    let done = {
        let (weak, stream) = (weak.clone(), stream.clone());
        CapturePreviewCompletedHandler::create(Box::new(move |result| {
            captured(&weak, screen, result.map(|()| stream).map_err(Into::into));
            Ok(())
        }))
    };
    // SAFETY: COM call on the thread that created the WebView; the handler
    // runs later on this thread's message loop.
    let started = unsafe {
        view.webview.CapturePreview(
            COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
            &stream,
            &done,
        )
    };
    if let Err(e) = started {
        captured(weak, screen, Err(e.into()));
    }
}

/// A still capture finished: decode it and hand the screen to the swapchain.
/// A failed capture shows black, never a stale frame.
fn captured(weak: &Weak<RefCell<State>>, screen: usize, png: Result<IStream>) {
    let Some(state) = weak.upgrade() else { return };
    let image = png.and_then(|stream| {
        // Capture size: no scaling (the screen size), within texture limits.
        let budget = Budget {
            fit: Fit::Stretch,
            screen: (u32::MAX, u32::MAX),
            max_dim: 16_384,
        };
        Decoder::new()?.decode_stream(&stream, budget)
    });
    let image = image
        .inspect_err(|e| log::warn!("web: capturing a still of screen {screen}: {e:#}"))
        .ok();
    let mut state = state.borrow_mut();
    let slot = &mut state.screens[screen];
    // Shown again (or reloaded) while capturing: the live page wins.
    if slot.visibility == Visibility::Still && matches!(slot.still, Still::Capturing) {
        slot.still = Still::ready(image);
    }
}

/// Sizes a controller to its window's client area.
fn fill(controller: &ICoreWebView2Controller, hwnd: HWND) {
    let mut rect = RECT::default();
    // SAFETY: valid window; `rect` outlives the call.
    unsafe {
        let _ = GetClientRect(hwnd, &mut rect);
        let _ = controller.SetBounds(rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(url: &str, theme: &str) -> Page {
        let theme_dir = PathBuf::from(theme);
        Page::resolve(url, &theme_dir.join(url), &theme_dir)
    }

    #[test]
    fn http_and_https_are_remote_in_any_case() {
        for url in [
            "https://example.com/a?b=c",
            "http://localhost:3000",
            "HTTPS://X.IO",
        ] {
            assert_eq!(resolve(url, r"C:\themes\t"), Page::Remote(url.to_owned()));
        }
        assert!(!is_remote("index.html"));
        assert!(!is_remote(r"C:\pages\index.html"));
        assert!(!is_remote("httpx://nope"));
    }

    #[test]
    fn page_inside_the_theme_serves_the_theme_folder() {
        assert_eq!(
            resolve("index.html", r"C:\themes\t"),
            Page::Local {
                folder: PathBuf::from(r"C:\themes\t"),
                url: "https://mypaper.theme/index.html".into(),
            }
        );
        assert_eq!(
            resolve("site/my page.html", r"C:\themes\t"),
            Page::Local {
                folder: PathBuf::from(r"C:\themes\t"),
                url: "https://mypaper.theme/site/my%20page.html".into(),
            }
        );
    }

    #[test]
    fn page_outside_the_theme_serves_its_own_folder() {
        let page = Page::resolve(
            r"D:\sites\fun\home.html",
            Path::new(r"D:\sites\fun\home.html"),
            Path::new(r"C:\themes\t"),
        );
        assert_eq!(
            page,
            Page::Local {
                folder: PathBuf::from(r"D:\sites\fun"),
                url: "https://mypaper.theme/home.html".into(),
            }
        );
    }

    #[test]
    fn percent_encoding_keeps_unreserved_and_escapes_the_rest() {
        assert_eq!(percent_encode("a-B_c.9~"), "a-B_c.9~");
        assert_eq!(percent_encode("a b#?é"), "a%20b%23%3F%C3%A9");
    }
}
