//! WebView2 for the bars: one shared environment, one controller per bar window, showing the
//! page from `https://bars.mypaper/` (`bars/shell/dist`) with images from their own virtual
//! hosts: `icons.` (the app icon cache), `packs.` (icon packs), `backdrop.` (wallpaper
//! snapshots) and `assets.bars.mypaper` (images `bars.json` names).
//!
//! WebView2 calls back on the main thread, from inside `DispatchMessage`; the callbacks only queue
//! a [`WebEvent`] for the main loop.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::Path;
use std::rc::Rc;

use anyhow::Result;
use mypaper_desktop::webview_env;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler, PermissionRequestedEventHandler, ProcessFailedEventHandler,
    TrySuspendCompletedHandler, WebMessageReceivedEventHandler, take_pwstr,
};
use windows::Win32::Foundation::{E_POINTER, HWND, RECT};
use windows::core::{HSTRING, Interface, PWSTR};

pub const PAGE_HOST: &str = "bars.mypaper";
pub const PAGE_URL: &str = "https://bars.mypaper/index.html";
const PAGE_PREFIX: &str = "https://bars.mypaper/";

/// What WebView2 callbacks report to the main loop.
pub enum WebEvent {
    Environment {
        generation: u64,
        result: windows::core::Result<ICoreWebView2Environment>,
    },
    Controller {
        bar: u32,
        result: windows::core::Result<ICoreWebView2Controller>,
    },
    Message {
        bar: u32,
        source: String,
        json: String,
    },
    ProcessFailed {
        bar: u32,
        kind: COREWEBVIEW2_PROCESS_FAILED_KIND,
    },
    /// The page answered [`Web::probe_paint`]: what it was sent is on screen.
    Painted { bar: u32 },
}

pub type Events = Rc<RefCell<VecDeque<WebEvent>>>;

/// Starts creating the environment, shared by every bar and with the widget host (one browser
/// process, docs/performance.md); the result arrives as [`WebEvent::Environment`].
pub fn create_environment(generation: u64, events: &Events) -> Result<()> {
    let events = events.clone();
    webview_env::create_environment(&webview_env::shared_data_dir()?, move |result| {
        events
            .borrow_mut()
            .push_back(WebEvent::Environment { generation, result });
    })
}

/// Asks `env` for a WebView in `hwnd`; it arrives as [`WebEvent::Controller`].
pub fn create_controller(
    env: &ICoreWebView2Environment,
    hwnd: HWND,
    bar: u32,
    events: &Events,
) -> Result<()> {
    let events = events.clone();
    let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
        move |result, controller| {
            let result = result.and_then(|()| controller.ok_or_else(|| E_POINTER.into()));
            events
                .borrow_mut()
                .push_back(WebEvent::Controller { bar, result });
            Ok(())
        },
    ));
    // SAFETY: our live window and a COM handler that outlives the call.
    unsafe { env.CreateCoreWebView2Controller(hwnd, &handler) }?;
    Ok(())
}

fn out_string(
    read: impl FnOnce(*mut PWSTR) -> windows::core::Result<()>,
) -> windows::core::Result<String> {
    let mut raw = PWSTR::null();
    read(&mut raw)?;
    Ok(take_pwstr(raw))
}

/// A bar's WebView.
pub struct Web {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
}

/// Where the page and its images come from. Each image folder is its own virtual host
/// (`https://<host>/`), loaded cross-origin by the page.
pub struct Folders<'a> {
    pub page: &'a Path,
    /// `(host, folder)`: the app icon cache, icon packs, wallpaper snapshots, copied images.
    pub images: &'a [(&'a str, &'a Path)],
}

impl Web {
    /// Configures a new WebView (transparent, locked to the page, no popups or prompts) and
    /// navigates to the page.
    pub fn attach(
        controller: ICoreWebView2Controller,
        bar: u32,
        scale: f64,
        size: (i32, i32),
        folders: &Folders,
        devtools: bool,
        events: &Events,
    ) -> Result<Self> {
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
                controller3.SetRasterizationScale(scale)?;
            }
            controller.SetBounds(bounds(size))?;
            controller.SetIsVisible(true)?;
            let webview = controller.CoreWebView2()?;

            let settings = webview.Settings()?;
            settings.SetIsScriptEnabled(true)?;
            settings.SetIsWebMessageEnabled(true)?;
            settings.SetAreDefaultScriptDialogsEnabled(false)?;
            settings.SetIsStatusBarEnabled(false)?;
            settings.SetAreDevToolsEnabled(devtools)?;
            settings.SetAreDefaultContextMenusEnabled(devtools)?;
            settings.SetIsZoomControlEnabled(false)?;
            settings.SetIsBuiltInErrorPageEnabled(false)?;
            if let Ok(settings) = settings.cast::<ICoreWebView2Settings3>() {
                settings.SetAreBrowserAcceleratorKeysEnabled(devtools)?;
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
                &HSTRING::from(PAGE_HOST),
                &HSTRING::from(plain(folders.page)),
                COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY,
            )?;
            // Icons and other images are plain files loaded cross-origin by the page.
            for (host, folder) in folders.images {
                std::fs::create_dir_all(folder)?;
                webview3.SetVirtualHostNameToFolderMapping(
                    &HSTRING::from(*host),
                    &HSTRING::from(plain(folder)),
                    COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_ALLOW,
                )?;
            }

            let mut token = 0i64;
            let events_for_messages = events.clone();
            webview.add_WebMessageReceived(
                &WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        let source = out_string(|p| args.Source(p))?;
                        let json = out_string(|p| args.WebMessageAsJson(p))?;
                        let event = if json == webview_env::PAINTED {
                            WebEvent::Painted { bar }
                        } else {
                            WebEvent::Message { bar, source, json }
                        };
                        events_for_messages.borrow_mut().push_back(event);
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            webview.add_NavigationStarting(
                &NavigationStartingEventHandler::create(Box::new(|_, args| {
                    if let Some(args) = args {
                        let url = out_string(|p| args.Uri(p))?;
                        if !url.starts_with(PAGE_PREFIX) {
                            log::warn!("blocked navigating a bar to {url}");
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
                        args.SetHandled(true)?;
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
            let events_for_failures = events.clone();
            webview.add_ProcessFailed(
                &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                        args.ProcessFailedKind(&mut kind)?;
                        events_for_failures
                            .borrow_mut()
                            .push_back(WebEvent::ProcessFailed { bar, kind });
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            webview.Navigate(&HSTRING::from(PAGE_URL))?;
            Ok(Self {
                controller,
                webview,
            })
        }
    }

    /// Sends a JSON message to the page.
    pub fn post(&self, json: &str) {
        // SAFETY: COM call on a live WebView.
        if let Err(e) = unsafe { self.webview.PostWebMessageAsJson(&HSTRING::from(json)) } {
            log::warn!("couldn't message a bar: {e}");
        }
    }

    /// Gives the page the keyboard (after the bar became the foreground window).
    pub fn focus(&self) {
        // SAFETY: COM call on a live controller.
        let _ = unsafe {
            self.controller
                .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC)
        };
    }

    /// Asks the page to answer ([`WebEvent::Painted`]) once what it was just sent is on screen.
    pub fn probe_paint(&self) {
        let script = HSTRING::from(webview_env::PAINT_PROBE);
        // SAFETY: COM call on a live WebView; no completion handler is needed.
        let _ = unsafe { self.webview.ExecuteScript(&script, None) };
    }

    pub fn reload(&self) {
        // SAFETY: COM call on a live WebView.
        let _ = unsafe { self.webview.Reload() };
    }

    /// Follows a window that moved, resized or changed scale.
    pub fn resize(&self, scale: f64, size: (i32, i32)) {
        // SAFETY: COM calls on a live controller.
        unsafe {
            if let Ok(controller3) = self.controller.cast::<ICoreWebView2Controller3>() {
                let _ = controller3.SetRasterizationScale(scale);
            }
            let _ = self.controller.SetBounds(bounds(size));
        }
    }

    /// Hidden: invisible and suspended (no rendering, no timers). Shown: the reverse.
    pub fn set_hidden(&self, hidden: bool) {
        // SAFETY: COM calls on a live WebView; suspending needs it invisible first.
        unsafe {
            let webview3 = self.webview.cast::<ICoreWebView2_3>();
            if hidden {
                let _ = self.controller.SetIsVisible(false);
                if let Ok(webview3) = &webview3 {
                    let done = TrySuspendCompletedHandler::create(Box::new(|_, _| Ok(())));
                    let _ = webview3.TrySuspend(&done);
                }
            } else {
                if let Ok(webview3) = &webview3 {
                    let _ = webview3.Resume();
                }
                let _ = self.controller.SetIsVisible(true);
            }
        }
    }
}

impl Drop for Web {
    fn drop(&mut self) {
        // SAFETY: COM call on a controller we own.
        let _ = unsafe { self.controller.Close() };
    }
}

fn bounds((width, height): (i32, i32)) -> RECT {
    RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    }
}

/// A path for WebView2 APIs: absolute, without the `\\?\` prefix.
pub fn plain(path: &Path) -> String {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\")
        .map(str::to_owned)
        .unwrap_or(text)
}
