//! One monitor's widget layer: a child window of the desktop, above the
//! icons, holding a transparent WebView2 that shows the shell page.
//!
//! WebView2 calls back on this thread, from inside `DispatchMessage`. The
//! callbacks here only queue a [`WebEvent`] for the main loop, except the
//! network filter, which must answer synchronously (see `net.rs`).

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::rc::Rc;

use anyhow::{Context, Result};
use mypaper_desktop::{DesktopHost, EXIT_RESTART, Monitor, startup, webview_env};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler,
    PermissionRequestedEventHandler, ProcessFailedEventHandler, TrySuspendCompletedHandler,
    WebMessageReceivedEventHandler, WebResourceRequestedEventHandler, take_pwstr,
};
use windows::Win32::Foundation::{E_POINTER, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DeleteObject, RGN_OR, SetWindowRgn,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW, IDC_ARROW,
    LoadCursorW, PostQuitMessage, RegisterClassExW, SW_HIDE, SW_SHOWNA, SetWindowLongPtrW,
    ShowWindow, WM_DESTROY, WM_ERASEBKGND, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN,
    WS_EX_NOACTIVATE,
};
use windows::core::{BOOL, HSTRING, Interface, PCWSTR, PWSTR, w};

use crate::net::{self, Origin, Party, Verdict, WidgetNet};

pub const SHELL_URL: &str = "https://shell.mypaper/index.html";
const SHELL_PREFIX: &str = "https://shell.mypaper/";
const CLASS: PCWSTR = w!("mypaper.widgets.surface");
/// Set on a surface window we destroy ourselves, so `WM_DESTROY` can tell
/// that apart from Explorer taking the desktop down.
const CLOSING: isize = 1;

/// What WebView2 callbacks report to the main loop.
pub enum WebEvent {
    Environment {
        generation: u64,
        result: windows::core::Result<ICoreWebView2Environment>,
    },
    Controller {
        surface: u32,
        result: windows::core::Result<ICoreWebView2Controller>,
    },
    ScriptAdded {
        surface: u32,
        id: Option<String>,
    },
    Message {
        surface: u32,
        source: String,
        json: String,
    },
    ProcessFailed {
        surface: u32,
        kind: COREWEBVIEW2_PROCESS_FAILED_KIND,
    },
}

pub type Events = Rc<RefCell<VecDeque<WebEvent>>>;

/// Which origins each widget may fetch from: read by every network filter.
pub type NetPolicy = Rc<RefCell<HashMap<String, WidgetNet>>>;

/// Everything a new WebView needs from the host.
pub struct Setup<'a> {
    pub env: &'a ICoreWebView2Environment,
    pub shell_dir: &'a Path,
    pub harden: &'a str,
    pub devtools: bool,
    pub policy: &'a NetPolicy,
    pub events: &'a Events,
}

pub fn register_class() -> Result<()> {
    // SAFETY: plain Win32 registration with a 'static window procedure.
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: GetModuleHandleW(None)?.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return Err(windows::core::Error::from_thread()).context("RegisterClassExW");
        }
    }
    Ok(())
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: standard window procedure, called by the system on our thread.
    unsafe {
        match msg {
            WM_ERASEBKGND => LRESULT(1), // the WebView paints everything
            WM_DESTROY => {
                if GetWindowLongPtrW(hwnd, GWLP_USERDATA) != CLOSING {
                    // Explorer took the desktop down: ask to be restarted.
                    log::warn!("the desktop went away; restarting");
                    PostQuitMessage(EXIT_RESTART.into());
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Starts creating the shared WebView2 environment; the result arrives as
/// [`WebEvent::Environment`].
pub fn create_environment(generation: u64, events: &Events) -> Result<()> {
    let data_dir = crate::files::local("webview2-widgets")?;
    std::fs::create_dir_all(&data_dir)?;
    let events = events.clone();
    let handler =
        CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |result, env| {
            let result = result.and_then(|()| env.ok_or_else(|| E_POINTER.into()));
            events
                .borrow_mut()
                .push_back(WebEvent::Environment { generation, result });
            Ok(())
        }));
    let options: ICoreWebView2EnvironmentOptions = CoreWebView2EnvironmentOptions::default().into();
    // SAFETY: valid strings and COM objects that outlive the call.
    unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            &HSTRING::from(crate::files::plain(&data_dir)),
            &options,
            &handler,
        )
    }
    .context("starting WebView2 (is the WebView2 Runtime installed?)")
}

/// Reads a COM-allocated string out-parameter.
fn out_string(
    read: impl FnOnce(*mut PWSTR) -> windows::core::Result<()>,
) -> windows::core::Result<String> {
    let mut raw = PWSTR::null();
    read(&mut raw)?;
    Ok(take_pwstr(raw))
}

/// Answers every request a WebView makes with the rule in `net.rs`.
fn filter_request(
    env: &ICoreWebView2Environment,
    policy: &NetPolicy,
    args: &ICoreWebView2WebResourceRequestedEventArgs,
) -> windows::core::Result<()> {
    // SAFETY: COM calls on live objects handed to us by WebView2.
    unsafe {
        let request = args.Request()?;
        let url = out_string(|p| request.Uri(p))?;
        let headers = request.Headers()?;
        let header = |name: PCWSTR| -> Option<String> {
            let mut present = BOOL::default();
            headers.Contains(name, &mut present).ok()?;
            if !present.as_bool() {
                return None;
            }
            out_string(|p| headers.GetHeader(name, p)).ok()
        };
        let origin = header(w!("Origin"));
        let referer = header(w!("Referer"));
        let service_worker = header(w!("Service-Worker")).is_some();
        let from = net::initiator(origin.as_deref(), referer.as_deref());
        let verdict = net::decide(&url, from.as_ref(), service_worker, |widget, target| {
            policy
                .borrow()
                .get(widget)
                .is_some_and(|net| net.origins.contains(target))
        });
        if verdict == Verdict::Block {
            let by = from
                .as_ref()
                .map_or("an unknown frame".to_owned(), ToString::to_string);
            log::info!("blocked {url} (from {by})");
            let response = env.CreateWebResourceResponse(
                None,
                403,
                w!("Forbidden"),
                w!("Content-Type: text/plain"),
            )?;
            return args.SetResponse(&response);
        }
        log::debug!("allowed {url} (from {from:?})");
        // Widget hosts aren't on the network: we serve them, CSP included.
        if let Some(Party::Widget(widget)) = Origin::of_url(&url).as_ref().map(Origin::party) {
            let (status, reason, headers, body) = widget_file(policy, widget, &url);
            let stream = SHCreateMemStream(Some(&body));
            let response = env.CreateWebResourceResponse(
                stream.as_ref(),
                status,
                &HSTRING::from(reason),
                &HSTRING::from(headers),
            )?;
            args.SetResponse(&response)?;
        }
    }
    Ok(())
}

/// A file from a widget's folder as `(status, reason, headers, body)`. Every
/// response carries the widget's Content-Security-Policy, so documents and
/// the workers they start can only connect where the manifest says.
fn widget_file(
    policy: &NetPolicy,
    widget: &str,
    url: &str,
) -> (i32, &'static str, String, Vec<u8>) {
    let policy = policy.borrow();
    let found = policy.get(widget).and_then(|net| {
        let path = net::widget_path(url)?;
        let body = std::fs::read(net.dir.join(&path)).ok()?;
        Some((net, path, body))
    });
    let Some((net, path, body)) = found else {
        return (
            404,
            "Not Found",
            "Content-Type: text/plain".into(),
            Vec::new(),
        );
    };
    let headers = format!(
        "Content-Type: {}\r\nContent-Security-Policy: {}\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-cache",
        net::content_type(&path),
        net::csp(&net.origins)
    );
    (200, "OK", headers, body)
}

struct Web {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
}

/// One monitor's window and WebView.
pub struct Surface {
    pub id: u32,
    pub monitor: Monitor,
    hwnd: HWND,
    web: Option<Web>,
    /// Asked the environment for a controller and waiting for it.
    pub creating: bool,
    /// The shell page said `ready`.
    pub ready: bool,
    pub hidden: bool,
    script: Option<String>,
    navigated: bool,
}

impl Surface {
    /// Creates the (still empty and click-through) window over `monitor`.
    pub fn new(id: u32, host: &DesktopHost, monitor: Monitor) -> Result<Self> {
        // SAFETY: class registered by `register_class`; the parent is a live shell window.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE,
                CLASS,
                w!("mypaper widgets"),
                WS_CHILD | WS_CLIPCHILDREN,
                0,
                0,
                monitor.rect.width,
                monitor.rect.height,
                Some(host.icons_parent()),
                None,
                None,
                None,
            )?
        };
        let surface = Self {
            id,
            monitor,
            hwnd,
            web: None,
            creating: false,
            ready: false,
            hidden: false,
            script: None,
            navigated: false,
        };
        surface.set_region(&[]);
        host.place_above_icons(hwnd, surface.monitor.rect)?;
        Ok(surface)
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn has_web(&self) -> bool {
        self.web.is_some()
    }

    fn scale(&self) -> f64 {
        f64::from(self.monitor.dpi) / 96.0
    }

    /// Asks `env` for this window's WebView; it arrives as [`WebEvent::Controller`].
    pub fn create_web(&mut self, env: &ICoreWebView2Environment, events: &Events) -> Result<()> {
        let (surface, events) = (self.id, events.clone());
        let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
            move |result, controller| {
                let result = result.and_then(|()| controller.ok_or_else(|| E_POINTER.into()));
                events
                    .borrow_mut()
                    .push_back(WebEvent::Controller { surface, result });
                Ok(())
            },
        ));
        // SAFETY: our live window and a COM handler that outlives the call.
        unsafe { env.CreateCoreWebView2Controller(self.hwnd, &handler) }?;
        self.creating = true;
        Ok(())
    }

    /// Configures a new WebView: transparent, locked down, filtered, with
    /// the shell folder mapped (widget hosts are served by the network
    /// filter) and the hardening script injected.
    /// The shell is navigated once that script is registered.
    pub fn attach(&mut self, controller: ICoreWebView2Controller, setup: &Setup) -> Result<()> {
        self.creating = false;
        // SAFETY: COM calls on the live controller and WebView we were just given.
        unsafe {
            let controller2: ICoreWebView2Controller2 = controller.cast()?;
            controller2.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR {
                A: 0,
                R: 0,
                G: 0,
                B: 0,
            })?;
            if let Ok(controller3) = controller.cast::<ICoreWebView2Controller3>() {
                controller3.SetShouldDetectMonitorScaleChanges(false)?;
                controller3.SetRasterizationScale(self.scale())?;
            }
            controller.SetBounds(self.bounds())?;
            controller.SetIsVisible(!self.hidden)?;
            let webview = controller.CoreWebView2()?;

            let settings = webview.Settings()?;
            settings.SetIsScriptEnabled(true)?;
            settings.SetIsWebMessageEnabled(true)?;
            settings.SetAreDefaultScriptDialogsEnabled(false)?;
            settings.SetIsStatusBarEnabled(false)?;
            settings.SetAreDevToolsEnabled(setup.devtools)?;
            settings.SetAreDefaultContextMenusEnabled(setup.devtools)?;
            settings.SetIsZoomControlEnabled(false)?;
            settings.SetIsBuiltInErrorPageEnabled(false)?;
            if let Ok(settings) = settings.cast::<ICoreWebView2Settings3>() {
                settings.SetAreBrowserAcceleratorKeysEnabled(setup.devtools)?;
            }
            if let Ok(settings) = settings.cast::<ICoreWebView2Settings4>() {
                settings.SetIsPasswordAutosaveEnabled(false)?;
                settings.SetIsGeneralAutofillEnabled(false)?;
            }
            if let Ok(settings) = settings.cast::<ICoreWebView2Settings5>() {
                settings.SetIsPinchZoomEnabled(false)?;
            }
            if let Ok(settings) = settings.cast::<ICoreWebView2Settings6>() {
                settings.SetIsSwipeNavigationEnabled(false)?;
            }

            let webview3: ICoreWebView2_3 = webview.cast()?;
            webview3.SetVirtualHostNameToFolderMapping(
                w!("shell.mypaper"),
                &HSTRING::from(crate::files::plain(setup.shell_dir)),
                COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY,
            )?;

            // Every request, from every frame and (where supported) worker.
            match webview.cast::<ICoreWebView2_22>() {
                Ok(webview22) => webview22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                    w!("*"),
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                )?,
                Err(_) => webview.AddWebResourceRequestedFilter(
                    w!("*"),
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                )?,
            }
            let mut token = 0i64;
            let (env, policy) = (setup.env.clone(), setup.policy.clone());
            webview.add_WebResourceRequested(
                &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args
                        && let Err(e) = filter_request(&env, &policy, &args)
                    {
                        log::warn!("network filter failed: {e}");
                    }
                    Ok(())
                })),
                &mut token,
            )?;

            let (surface, events) = (self.id, setup.events.clone());
            webview.add_WebMessageReceived(
                &WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        let source = out_string(|p| args.Source(p))?;
                        let json = out_string(|p| args.WebMessageAsJson(p))?;
                        if json == webview_env::PAINTED {
                            startup::ready("widgets", "first_paint_ms");
                            return Ok(());
                        }
                        events.borrow_mut().push_back(WebEvent::Message {
                            surface,
                            source,
                            json,
                        });
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            // The top-level document is the shell, always.
            webview.add_NavigationStarting(
                &NavigationStartingEventHandler::create(Box::new(|_, args| {
                    if let Some(args) = args {
                        let url = out_string(|p| args.Uri(p))?;
                        if !url.starts_with(SHELL_PREFIX) {
                            log::warn!("blocked navigating the shell to {url}");
                            args.SetCancel(true)?;
                        }
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            webview.add_NewWindowRequested(
                &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                    if let Some(args) = args {
                        args.SetHandled(true)?; // no popups
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            webview.add_PermissionRequested(
                &PermissionRequestedEventHandler::create(Box::new(|_, args| {
                    if let Some(args) = args {
                        args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            let (surface, events) = (self.id, setup.events.clone());
            webview.add_ProcessFailed(
                &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                        args.ProcessFailedKind(&mut kind)?;
                        events
                            .borrow_mut()
                            .push_back(WebEvent::ProcessFailed { surface, kind });
                    }
                    Ok(())
                })),
                &mut token,
            )?;

            self.web = Some(Web {
                controller,
                webview,
            });
        }
        self.add_script(setup.harden, setup.events)?;
        if self.hidden {
            self.hidden = false;
            self.set_hidden(true);
        }
        Ok(())
    }

    fn bounds(&self) -> RECT {
        RECT {
            left: 0,
            top: 0,
            right: self.monitor.rect.width,
            bottom: self.monitor.rect.height,
        }
    }

    /// Registers the hardening script for every future document; the shell
    /// (re)loads once that's done (see [`on_script_added`](Self::on_script_added)).
    fn add_script(&mut self, script: &str, events: &Events) -> Result<()> {
        let Some(web) = &self.web else { return Ok(()) };
        let (surface, events) = (self.id, events.clone());
        let handler = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(
            move |result, id| {
                let id = result.ok().map(|()| id);
                events
                    .borrow_mut()
                    .push_back(WebEvent::ScriptAdded { surface, id });
                Ok(())
            },
        ));
        // SAFETY: COM call on a live WebView with a handler that outlives it.
        unsafe {
            web.webview
                .AddScriptToExecuteOnDocumentCreated(&HSTRING::from(script), &handler)
        }?;
        Ok(())
    }

    /// Swaps in a rebuilt hardening script and reloads the shell.
    pub fn replace_script(&mut self, script: &str, events: &Events) -> Result<()> {
        if let (Some(web), Some(id)) = (&self.web, self.script.take()) {
            // SAFETY: COM call on a live WebView; `id` came from AddScriptToExecuteOnDocumentCreated.
            unsafe {
                web.webview
                    .RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(id))
            }?;
        }
        self.add_script(script, events)
    }

    pub fn on_script_added(&mut self, id: Option<String>) {
        let Some(web) = &self.web else { return };
        let Some(id) = id else {
            return log::error!("couldn't register the hardening script");
        };
        self.script = Some(id);
        self.ready = false;
        // SAFETY: COM calls on a live WebView.
        let loaded = unsafe {
            if self.navigated {
                web.webview.Reload()
            } else {
                web.webview.Navigate(&HSTRING::from(SHELL_URL))
            }
        };
        match loaded {
            Ok(()) => self.navigated = true,
            Err(e) => log::error!("couldn't load the shell: {e}"),
        }
    }

    /// Reloads the shell page (after its renderer crashed).
    pub fn reload(&mut self) {
        self.ready = false;
        if let Some(web) = &self.web {
            // SAFETY: COM call on a live WebView.
            let _ = unsafe { web.webview.Reload() };
        }
    }

    /// Drops the WebView (its browser process died); a new one can be attached.
    pub fn drop_web(&mut self) {
        if let Some(web) = self.web.take() {
            // SAFETY: COM call on a controller we own.
            let _ = unsafe { web.controller.Close() };
        }
        self.creating = false;
        self.ready = false;
        self.navigated = false;
        self.script = None;
    }

    /// Sends a JSON message to the shell page.
    pub fn post(&self, json: &str) {
        if let Some(web) = &self.web {
            // SAFETY: COM call on a live WebView.
            if let Err(e) = unsafe { web.webview.PostWebMessageAsJson(&HSTRING::from(json)) } {
                log::warn!("couldn't message the shell: {e}");
            }
        }
    }

    /// Asks the shell to answer once what it was just sent is on screen (the first-paint
    /// marker, docs/performance.md).
    pub fn probe_paint(&self) {
        if let Some(web) = &self.web {
            let script = HSTRING::from(webview_env::PAINT_PROBE);
            // SAFETY: COM call on a live WebView; no completion handler is needed.
            let _ = unsafe { web.webview.ExecuteScript(&script, None) };
        }
    }

    /// Makes only `rects` (window pixels: left, top, right, bottom) hit-testable
    /// and visible; everything else clicks through to the desktop.
    pub fn set_region(&self, rects: &[[i32; 4]]) {
        // SAFETY: GDI regions we create; SetWindowRgn takes ownership of `region`.
        unsafe {
            let region = CreateRectRgn(0, 0, 0, 0);
            for &[left, top, right, bottom] in rects {
                let part = CreateRectRgn(left, top, right, bottom);
                CombineRgn(Some(region), Some(region), Some(part), RGN_OR);
                let _ = DeleteObject(part.into());
            }
            SetWindowRgn(self.hwnd, Some(region), true);
        }
    }

    /// The whole window, for edit mode.
    pub fn full_region(&self) -> [i32; 4] {
        [0, 0, self.monitor.rect.width, self.monitor.rect.height]
    }

    /// Hidden: the window, the WebView, and its renderer (suspended). Shown:
    /// the reverse.
    pub fn set_hidden(&mut self, hidden: bool) {
        if self.hidden == hidden {
            return;
        }
        self.hidden = hidden;
        if let Some(web) = &self.web {
            // SAFETY: COM calls on a live WebView; suspend needs it invisible first.
            unsafe {
                let webview3 = web.webview.cast::<ICoreWebView2_3>();
                let memory = web.webview.cast::<ICoreWebView2_19>();
                if hidden {
                    let _ = web.controller.SetIsVisible(false);
                    if let Ok(webview3) = &webview3 {
                        let done = TrySuspendCompletedHandler::create(Box::new(|_, _| Ok(())));
                        let _ = webview3.TrySuspend(&done);
                    }
                    if let Ok(memory) = &memory {
                        let _ = memory
                            .SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW);
                    }
                } else {
                    if let Ok(memory) = &memory {
                        let _ = memory.SetMemoryUsageTargetLevel(
                            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
                        );
                    }
                    if let Ok(webview3) = &webview3 {
                        let _ = webview3.Resume();
                    }
                    let _ = web.controller.SetIsVisible(true);
                }
            }
        }
        // SAFETY: our window; SW_SHOWNA never activates it.
        let _ = unsafe { ShowWindow(self.hwnd, if hidden { SW_HIDE } else { SW_SHOWNA }) };
    }

    /// Follows a monitor that moved, resized or changed scale.
    pub fn set_monitor(&mut self, host: &DesktopHost, monitor: Monitor) -> Result<()> {
        self.monitor = monitor;
        host.place_above_icons(self.hwnd, self.monitor.rect)?;
        if self.hidden {
            // place_above_icons shows the window; keep it hidden.
            // SAFETY: our window.
            let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        }
        if let Some(web) = &self.web {
            // SAFETY: COM calls on a live controller.
            unsafe {
                if let Ok(controller3) = web.controller.cast::<ICoreWebView2Controller3>() {
                    controller3.SetRasterizationScale(self.scale())?;
                }
                web.controller.SetBounds(self.bounds())?;
            }
        }
        Ok(())
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.drop_web();
        // SAFETY: our window, destroyed on its creating thread; the flag tells
        // its WM_DESTROY this isn't Explorer going away.
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, CLOSING);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
