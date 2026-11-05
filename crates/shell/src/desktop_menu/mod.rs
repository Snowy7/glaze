//! mypaper's own right-click menu for the bare desktop (on by default, `desktop_menu.enabled`;
//! docs/desktop-extras.md).
//!
//! **Two threads, both ours; nothing is injected anywhere.**
//!
//! - The **hook thread** ([`hook`]) owns a `WH_MOUSE_LL` hook and swallows right-clicks on the
//!   bare desktop, posting each to the shell's [inbox](crate::inbox).
//! - The shell's **UI thread** does the slow work here: it reads the icon rectangles from the
//!   desktop's folder view, confirms each swallowed click with UI Automation
//!   (`ElementFromPoint`: a list *item* is an icon), opens the menu ([`mypaper_ui::Menu`]) and
//!   runs the chosen command. If the confirmation says "icon" (the cached rectangles were
//!   stale), the click is replayed to the native desktop. "Show more options" (only in a
//!   configured menu) does the same: the right-click is replayed, marked as ours so the hook
//!   lets it through once.
//! - The **shell menu worker** ([`shell_menu`]) holds the desktop folder's own context menu
//!   (New, the shell extensions), built ahead of time so the menu opens without waiting for it.
//!
//! The hook is not installed at all while a fullscreen app is in front, the session is locked or
//! the display is off (`Shift`+right-click is always native).

mod hook;
pub mod items;
pub mod shell_menu;

use std::process::Command as Process;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use mypaper_config::DesktopMenu as MenuConfig;
use mypaper_desktop::desktop_hit::IconMap;
use mypaper_desktop::desktop_view::{self, DesktopView, SortKey};
use mypaper_desktop::hold::Fact;
use mypaper_desktop::{ControlWindow, WM_FACT, fullscreen_app_in_front};
use mypaper_ui::{Event, Image, Menu, Outcome, SurfaceId, Tokens, Ui, screen_at};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, HWINEVENTHOOK, IUIAutomation, SetWinEventHook, UIA_ListItemControlTypeId,
    UnhookWinEvent,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, KillTimer, SW_SHOWNORMAL, SetTimer,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_APP, WM_TIMER,
};
use windows::core::{HSTRING, PCWSTR, w};

use self::hook::{HookThread, ICONS, ICONS_DIRTY_QUEUED, MENU_OPEN, replay_right_click, unpack};
use self::items::{Command, Sources, ViewState};
use self::shell_menu::ShellMenu;
use crate::apps;
use crate::inbox::{self, Msg};

// Inbox messages of the desktop menu.
const WM_RECHECK: u32 = WM_APP + 40;
const WM_REQUEST: u32 = WM_APP + 41;
const WM_RELEASE: u32 = WM_APP + 42;
const WM_OUTSIDE: u32 = WM_APP + 43;
const WM_ICONS_DIRTY: u32 = WM_APP + 44;

/// A refresh of the icon rectangles this long after the last click on the desktop (long enough
/// for Explorer to have moved the icon).
const ICONS_SETTLE_MS: u32 = 300;
const ICONS_TIMER: usize = 1;

/// A foreground change is already queued for a hook recheck.
static RECHECK_QUEUED: AtomicBool = AtomicBool::new(false);

/// Everything the menu takes from the config and the theme. Replaced as a whole by
/// [`DesktopMenu::update`]; read each time the menu opens.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub menu: MenuConfig,
    pub tokens: Tokens,
}

/// A swallowed press waiting for its release.
struct Pending {
    at: (i32, i32),
    /// The UI Automation verdict: nothing but desktop at that point.
    bare: bool,
    /// The desktop's state for the check marks, read while the button is still down so that
    /// opening the menu on release does not wait for Explorer.
    state: ViewState,
}

/// The running desktop menu; dropping it removes the hook and destroys the menu's windows.
pub struct DesktopMenu {
    settings: Settings,
    /// The menu's look: the top bar's theme (the kit's menu tokens), read on the first open after
    /// the settings change, so the desktop, dock and tray menus and the bar look alike.
    themed: Option<Tokens>,
    menu: Menu<Command>,
    /// The shell's own menu (New, extensions); `None` if its worker could not start.
    shell: Option<ShellMenu>,
    /// mypaper's mark, for its own items only where no icon font has their glyphs.
    icon: Option<Arc<Image>>,
    uia: Option<IUIAutomation>,
    hook: HookThread,
    pending: Option<Pending>,
    locked: bool,
    display_off: bool,
    hook_on: bool,
    /// Where the menu was opened: "Show more options" replays the click there.
    opened_at: (i32, i32),
    /// An icon refresh is scheduled (a timer on the inbox window).
    icons_timer: bool,
    inbox: HWND,
    _control: Option<ControlWindow>,
    foreground: Option<HWINEVENTHOOK>,
}

impl DesktopMenu {
    /// Starts the hook thread and makes the menu's windows; `inbox` is the shell's inbox window.
    pub fn start(settings: Settings, inbox: HWND) -> Result<Self> {
        let uia = create_uia()
            .inspect_err(|e| {
                log::warn!("desktop menu: no UI Automation, trusting the icon cache: {e:#}")
            })
            .ok();
        let control = ControlWindow::new(w!("mypaper.desktopmenu.control"))
            .inspect_err(|e| log::warn!("desktop menu: no lock/display notifications: {e:#}"))
            .ok();
        let icon_px = icon_pixels(&settings.tokens, screen_at(0, 0).dpi);
        let shell = ShellMenu::start(icon_px)
            .inspect_err(|e| log::warn!("desktop menu: no shell extensions: {e:#}"))
            .ok();
        let icon = mark(icon_px as u32);
        let mut menu = Self {
            settings,
            themed: None,
            menu: Menu::new("mypaper desktop menu")?,
            shell,
            icon,
            uia,
            hook: HookThread::spawn()?,
            pending: None,
            locked: false,
            display_off: false,
            hook_on: false,
            opened_at: (0, 0),
            icons_timer: false,
            inbox,
            _control: control,
            foreground: watch_foreground(),
        };
        menu.refresh_icons("start");
        menu.sync_hook();
        Ok(menu)
    }

    /// New items or look, used from the next time the menu opens.
    pub fn update(&mut self, settings: Settings) {
        self.settings = settings;
        self.themed = None;
    }

    /// An inbox message for the menu (or a `WM_FACT` from the control window).
    pub fn handle(&mut self, ui: &mut Ui, msg: Msg) {
        match msg.message {
            WM_REQUEST => {
                let at = unpack(msg.lparam);
                let bare = self.confirm_bare(at);
                self.pending = Some(Pending {
                    at,
                    bare,
                    state: if bare {
                        view_state()
                    } else {
                        ViewState::default()
                    },
                });
            }
            WM_RELEASE => {
                if let Some(pending) = self.pending.take() {
                    if pending.bare {
                        self.show(ui, pending.at, pending.state);
                    } else {
                        log::debug!(
                            "desktop menu: stale icon cache; replaying the click to the desktop"
                        );
                        replay_right_click(pending.at);
                        self.refresh_icons("a stale cache");
                    }
                }
            }
            WM_OUTSIDE => {
                let (x, y) = unpack(msg.lparam);
                if !self.menu.contains(x, y) {
                    self.menu.close();
                    MENU_OPEN.store(false, Ordering::Relaxed);
                }
            }
            WM_RECHECK => {
                RECHECK_QUEUED.store(false, Ordering::Relaxed);
                self.sync_hook();
            }
            WM_FACT => match Fact::from_wparam(msg.wparam) {
                Some(Fact::SessionLocked) => {
                    self.locked = msg.lparam != 0;
                    self.sync_hook();
                }
                Some(Fact::DisplayOff) => {
                    self.display_off = msg.lparam != 0;
                    self.sync_hook();
                }
                _ => {}
            },
            WM_ICONS_DIRTY => {
                // Debounce: the refresh runs once the clicking has stopped.
                // SAFETY: a timer on the inbox window; replacing it restarts the wait.
                self.icons_timer =
                    unsafe { SetTimer(Some(self.inbox), ICONS_TIMER, ICONS_SETTLE_MS, None) } != 0;
            }
            WM_TIMER if msg.wparam == ICONS_TIMER && self.icons_timer => {
                // SAFETY: kills the timer set above.
                let _ = unsafe { KillTimer(Some(self.inbox), ICONS_TIMER) };
                self.icons_timer = false;
                ICONS_DIRTY_QUEUED.store(false, Ordering::Relaxed);
                self.refresh_icons("a click on the desktop");
            }
            _ => {}
        }
    }

    /// Input for one of the menu's windows. Returns whether it was the menu's.
    pub fn on_event(&mut self, ui: &mut Ui, id: SurfaceId, event: &Event) -> bool {
        if !self.menu.owns(id) {
            return false;
        }
        match self.menu.handle(ui, id, event) {
            Some(Outcome::Chosen(command)) => self.run(command),
            Some(Outcome::Closed) => MENU_OPEN.store(false, Ordering::Relaxed),
            None => {}
        }
        true
    }

    /// Installs the hook unless a fullscreen app is in front, the session is locked or the
    /// display is off.
    fn sync_hook(&mut self) {
        let wanted = !self.locked && !self.display_off && !fullscreen_app_in_front();
        if wanted != self.hook_on {
            self.hook_on = wanted;
            self.hook.set(wanted);
            log::debug!("desktop menu: hook {}", if wanted { "on" } else { "off" });
            if wanted {
                self.refresh_icons("the hook coming on");
            }
        }
    }

    /// Re-reads the icons' rectangles for the hook's fast check.
    fn refresh_icons(&mut self, why: &str) {
        let map = match DesktopView::open().and_then(|view| view.icon_rects()) {
            Ok(rects) => {
                log::debug!("desktop menu: {} icons (after {why})", rects.len());
                IconMap::new(rects)
            }
            Err(e) => {
                log::debug!("desktop menu: icon rectangles unavailable: {e:#}");
                IconMap::empty()
            }
        };
        if let Ok(mut icons) = ICONS.write() {
            *icons = map;
        }
    }

    /// Is there nothing but desktop at `at`? An icon is a UI Automation list *item*. Without an
    /// answer, the cached rectangles (which already said "bare") stand.
    fn confirm_bare(&self, at: (i32, i32)) -> bool {
        let Some(uia) = &self.uia else { return true };
        let started = std::time::Instant::now();
        // SAFETY: documented UI Automation calls on an interface we own.
        let verdict = unsafe {
            uia.ElementFromPoint(POINT { x: at.0, y: at.1 })
                .and_then(|element| element.CurrentControlType())
        };
        log::debug!(
            "desktop menu: UI Automation answered in {:?}",
            started.elapsed()
        );
        match verdict {
            Ok(kind) => kind != UIA_ListItemControlTypeId,
            Err(e) => {
                log::debug!("desktop menu: ElementFromPoint failed: {e}");
                true
            }
        }
    }

    fn show(&mut self, ui: &mut Ui, at: (i32, i32), state: ViewState) {
        let started = std::time::Instant::now();
        self.opened_at = at;
        let screen = screen_at(at.0, at.1);
        let snapshot = self.shell.as_mut().and_then(|shell| {
            shell.set_icon_size(icon_pixels(&self.settings.tokens, screen.dpi));
            shell.snapshot()
        });
        let plugins = crate::widgets::plugin_events::menu_items();
        let sources = Sources {
            plugins: &plugins,
            shell: snapshot.as_ref(),
            // Glyphs first; mypaper's mark only where neither icon font is installed.
            mypaper: self
                .icon
                .as_ref()
                .filter(|_| {
                    !ui.text.has_family(mypaper_ui::icons::SYSTEM_FAMILY)
                        && !ui.text.has_family("Segoe MDL2 Assets")
                }),
        };
        let items = items::build_with(&self.settings.menu, state, sources);
        if items.is_empty() {
            replay_right_click(at);
            return;
        }
        MENU_OPEN.store(true, Ordering::Relaxed);
        let scale = self.settings.tokens.scale;
        let base = self
            .themed
            .get_or_insert_with(|| Tokens {
                scale,
                ..crate::bars::look::menu_tokens(&ui.text)
            })
            .clone();
        // No outlines on glass: the engine draws its one edge and its shadow.
        let glass = base.glass.is_some() || mypaper_ui::glass::style().is_some();
        let tokens = Tokens {
            border: base.border && !glass,
            shadow: base.shadow && !glass,
            ..base
        };
        if let Err(e) = self.menu.open(ui, items, at, screen, &tokens) {
            log::warn!("desktop menu: {e:#}");
            MENU_OPEN.store(false, Ordering::Relaxed);
        }
        log::info!(
            "desktop menu: open in {:.1?} ({})",
            started.elapsed(),
            if snapshot.is_some() {
                "with the shell's items"
            } else {
                "built-ins only: the shell's items are not ready"
            }
        );
    }

    /// Runs a chosen command (the menu is already closed).
    fn run(&mut self, command: Command) {
        MENU_OPEN.store(false, Ordering::Relaxed);
        if let Err(e) = self.try_run(command) {
            log::warn!("desktop menu: {e:#}");
        }
    }

    fn try_run(&mut self, command: Command) -> Result<()> {
        match command {
            Command::IconSize(size) => DesktopView::open()?.set_icon_size(size)?,
            Command::ToggleIcons => {
                let view = DesktopView::open()?;
                view.set_icons_visible(!view.icons_visible()?)?;
            }
            Command::AutoArrange(on) => DesktopView::open()?.set_arrangement(false, on)?,
            Command::AlignToGrid(on) => DesktopView::open()?.set_arrangement(true, on)?,
            Command::Shell { generation, id } => {
                if let Some(shell) = &mut self.shell {
                    shell.invoke(generation, id, self.opened_at);
                }
            }
            // The widget gallery is Studio's.
            Command::AddWidget => apps::open_studio(),
            Command::SortBy(key) => {
                let view = DesktopView::open()?;
                let ascending = view.sort()?.is_none_or(|(_, up)| up);
                view.set_sort(key, ascending)?;
            }
            Command::SortOrder { ascending } => {
                let view = DesktopView::open()?;
                let key = view.sort()?.map_or(SortKey::Name, |(key, _)| key);
                view.set_sort(key, ascending)?;
            }
            Command::Refresh => DesktopView::open()?.refresh()?,
            Command::New(item) => DesktopView::open()?.create(item)?,
            Command::Paste => DesktopView::open()?.paste()?,
            Command::OpenStudio => apps::open_studio(),
            Command::EditWidgets => apps::toggle_widget_edit_mode(),
            Command::Display => shell_open("ms-settings:display", None)?,
            Command::Personalize => shell_open("ms-settings:personalization", None)?,
            Command::Terminal => open_terminal()?,
            Command::MoreOptions => replay_right_click(self.opened_at),
            Command::Custom { command, args } => run_custom(&command, &args)?,
            Command::Plugin { plugin, id } => crate::widgets::plugin_events::invoke(&plugin, &id),
        }
        // The view changed (or may have): the icons may have moved.
        self.refresh_icons("a menu command");
        Ok(())
    }
}

impl Drop for DesktopMenu {
    fn drop(&mut self) {
        MENU_OPEN.store(false, Ordering::Relaxed);
        if self.icons_timer {
            // SAFETY: kills our own timer.
            let _ = unsafe { KillTimer(Some(self.inbox), ICONS_TIMER) };
        }
        if let Some(foreground) = self.foreground.take() {
            // SAFETY: a hook we installed.
            let _ = unsafe { UnhookWinEvent(foreground) };
        }
    }
}

/// Foreground changes re-decide whether the hook should be on (fullscreen apps).
fn watch_foreground() -> Option<HWINEVENTHOOK> {
    let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
    // SAFETY: out-of-context hook with a 'static callback; events arrive through this thread's loop.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_MINIMIZEEND,
            None,
            Some(on_foreground),
            0,
            0,
            flags,
        )
    };
    (!hook.is_invalid()).then_some(hook)
}

unsafe extern "system" fn on_foreground(
    _: HWINEVENTHOOK,
    _: u32,
    _: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    // Only whole windows count; carets and cursors fire constantly.
    if object == 0 && child == 0 && !RECHECK_QUEUED.swap(true, Ordering::Relaxed) {
        inbox::post(WM_RECHECK, 0);
    }
}

fn view_state() -> ViewState {
    let clipboard = clipboard_has_files();
    match DesktopView::open() {
        Ok(view) => {
            let (auto_arrange, align_to_grid) = view.arrangement().unwrap_or_default();
            ViewState {
                icon_size: view.icon_size().ok(),
                icons_visible: view.icons_visible().unwrap_or(true),
                auto_arrange,
                align_to_grid,
                sort: view.sort().ok().flatten(),
                can_paste: clipboard,
            }
        }
        Err(e) => {
            log::debug!("desktop menu: view state unavailable: {e:#}");
            ViewState {
                icons_visible: true,
                can_paste: clipboard,
                ..ViewState::default()
            }
        }
    }
}

/// mypaper's mark (`assets/logo/mark-small.svg`, drawn for 16-24 px) `px` square, premultiplied
/// BGRA.
fn mark(px: u32) -> Option<Arc<Image>> {
    let svg = include_bytes!("../../../../assets/logo/mark-small.svg");
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()).ok()?;
    let scale = px as f32 / tree.size().width().max(tree.size().height());
    let mut pixmap = resvg::tiny_skia::Pixmap::new(px, px)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia is premultiplied RGBA; the kit's images are premultiplied BGRA.
    let mut pixels = pixmap.take();
    for p in pixels.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    Some(Arc::new(Image {
        width: px,
        height: px,
        pixels,
    }))
}

/// The menu's icon size in device pixels on a monitor of `dpi`: 16 px, scaled.
fn icon_pixels(tokens: &Tokens, dpi: u32) -> i32 {
    (mypaper_ui::menu::ICON_SIZE * dpi.max(96) as f32 / 96.0 * tokens.scale.clamp(0.5, 2.0)).round()
        as i32
}

fn clipboard_has_files() -> bool {
    use windows::Win32::System::DataExchange::IsClipboardFormatAvailable;
    // CF_HDROP: files copied or cut in Explorer.
    // SAFETY: no preconditions.
    unsafe { IsClipboardFormatAvailable(15) }.is_ok()
}

fn create_uia() -> Result<IUIAutomation> {
    // SAFETY: documented COM activation, in the apartment this thread set up.
    Ok(unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }?)
}

/// `ShellExecuteW("open", file, params)`: documents, URIs like `ms-settings:`, programs.
fn shell_open(file: &str, params: Option<&str>) -> Result<()> {
    let file = HSTRING::from(file);
    let params = params.map(HSTRING::from);
    // SAFETY: the strings outlive the call.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &file,
            params
                .as_ref()
                .map_or(PCWSTR::null(), |p| PCWSTR(p.as_ptr())),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute reports success as a value above 32.
    if result.0 as isize <= 32 {
        anyhow::bail!("could not open {file}");
    }
    Ok(())
}

/// Windows Terminal in the Desktop folder, or a console where it is not installed.
fn open_terminal() -> Result<()> {
    let desktop = desktop_view::desktop_folder()?;
    let quoted = format!("\"{}\"", desktop.display());
    shell_open("wt.exe", Some(&format!("-d {quoted}")))
        .or_else(|_| shell_open("cmd.exe", Some(&format!("/K cd /d {quoted}"))))
}

/// Runs a custom command. A program that cannot be started directly (a document, a URI) goes to
/// the shell.
fn run_custom(command: &str, args: &[String]) -> Result<()> {
    match Process::new(command).args(args).spawn() {
        Ok(_) => Ok(()),
        Err(direct) => {
            let joined = args
                .iter()
                .map(|a| format!("\"{a}\""))
                .collect::<Vec<_>>()
                .join(" ");
            shell_open(command, (!joined.is_empty()).then_some(joined.as_str()))
                .with_context(|| format!("starting {command}: {direct}"))
        }
    }
}
