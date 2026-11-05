//! The tray icon (the mypaper logo) and its right-click menu: pause, theme
//! picker, widget edit mode, the shell bars (`bars.json`), studio, restart
//! and quit. Everything goes through `config.json` / `bars.json` (the
//! processes apply them live) or a message to the supervisor loop, so the
//! tray holds no state of its own.

use mypaper_bars::tray_menu as wire;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use anyhow::{Context, Result};
use mypaper_bars::{BarsConfig, TaskbarMode};
use mypaper_config::Config;
use mypaper_desktop::desktop_view::IconSize;
use mypaper_desktop::{ConfigWatcher, watch_config};
use mypaper_theme::LoadedTheme;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DestroyWindow, FindWindowExW, HICON, HMENU, HWND_MESSAGE,
    IDI_APPLICATION, LR_DEFAULTCOLOR, LoadIconW, MENU_ITEM_FLAGS, MF_CHECKED, MF_GRAYED, MF_POPUP,
    MF_SEPARATOR, MF_STRING, PostMessageW, PostQuitMessage, PostThreadMessageW, RegisterClassExW,
    RegisterWindowMessageW, SM_CXSMICON, SetForegroundWindow, TPM_BOTTOMALIGN, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_CONTEXTMENU, WM_NULL, WNDCLASSEXW,
    WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::desktop_extras;

/// Tray icon callback (mouse and keyboard events on the icon).
const WM_TRAY: u32 = WM_APP + 1;
/// Posted by the config watcher: refresh the tooltip.
const WM_CONFIG_CHANGED: u32 = WM_APP + 2;
/// Posted to the supervisor thread by "Restart wallpaper".
pub const WM_RESTART_WALLPAPER: u32 = WM_APP + 3;

const CLASS: PCWSTR = w!("mypaper.tray");
const ICON_ID: u32 = 1;
const STUDIO_EXE: &str = "mypaper-studio.exe";

const ID_PAUSE: usize = 1;
const ID_STUDIO: usize = 2;
const ID_RESTART: usize = 3;
const ID_QUIT: usize = 4;
const ID_EDIT_WIDGETS: usize = 5;
const ID_TOP_BAR: usize = 6;
const ID_DOCK: usize = 7;
const ID_TASKBAR_KEEP: usize = 8;
const ID_TASKBAR_AUTOHIDE: usize = 9;
const ID_TASKBAR_HIDE: usize = 10;
const ID_DESKTOP_ICONS: usize = 11;
const ID_ICONS_LARGE: usize = 12;
const ID_ICONS_MEDIUM: usize = 13;
const ID_ICONS_SMALL: usize = 14;
const ID_DESKTOP_MENU: usize = 15;
const ID_CLASSIC_MENU: usize = 16;
const ID_OVERLAY_WIDGETS: usize = 17;
/// Theme `i` in the Themes submenu has id `ID_THEME + i`.
const ID_THEME: usize = 100;

// The window procedure also keeps a command snapshot until the shell replies.
thread_local! { static PENDING_MENU: RefCell<Option<(usize, Vec<String>)>> = const { RefCell::new(None) }; }
static NONCE: AtomicUsize = AtomicUsize::new(1);

static ICON: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// The tray icon. Removed from the tray on drop.
pub struct Tray {
    hwnd: HWND,
    watcher: Option<ConfigWatcher>,
}

impl Tray {
    /// Creates the (never shown) tray window and adds the icon. A missing
    /// Explorer is fine: the icon appears when `TaskbarCreated` arrives.
    pub fn new() -> Result<Self> {
        // SAFETY: plain Win32 registration and window creation with a 'static wndproc and class name.
        let hwnd = unsafe {
            TASKBAR_CREATED.store(
                RegisterWindowMessageW(w!("TaskbarCreated")),
                Ordering::Relaxed,
            );
            let hinstance = GetModuleHandleW(None)?.into();
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance,
                lpszClassName: CLASS,
                ..Default::default()
            };
            if RegisterClassExW(&class) == 0 {
                return Err(windows::core::Error::from_thread()).context("RegisterClassExW");
            }
            // A hidden top-level window (not message-only), so it gets the
            // TaskbarCreated broadcast.
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                CLASS,
                w!("mypaper tray"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(hinstance),
                None,
            )?
        };
        ICON.store(make_icon().0 as isize, Ordering::Relaxed);
        add_icon(hwnd);

        // Keep the tooltip in step with theme changes made elsewhere (studio).
        let target = hwnd.0 as isize;
        let watcher = watch_config(move || {
            // SAFETY: posting to a window from another thread is allowed;
            // it fails harmlessly once the window is gone.
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(target as _)),
                    WM_CONFIG_CHANGED,
                    WPARAM(0),
                    LPARAM(0),
                )
            };
        })
        .inspect_err(|e| log::warn!("tray tooltip will not follow config changes: {e:#}"))
        .ok();
        Ok(Self { hwnd, watcher })
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.watcher.take(); // stop posting to the window before it dies
        let nid = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: ICON_ID,
            ..Default::default()
        };
        // SAFETY: removes the icon we added, then destroys our own window and icon.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            let _ = DestroyWindow(self.hwnd);
            let _ = DestroyIcon(HICON(ICON.load(Ordering::Relaxed) as _));
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY if (lparam.0 & 0xFFFF) as u32 == WM_CONTEXTMENU => {
            // With NOTIFYICON_VERSION_4, wParam is the anchor point (signed 16-bit halves).
            let x = i32::from(wparam.0 as u16 as i16);
            let y = i32::from((wparam.0 >> 16) as u16 as i16);
            show_menu(hwnd, x, y);
            LRESULT(0)
        }
        wire::CHOSEN => {
            let themes = PENDING_MENU.with_borrow_mut(|pending| {
                if pending.as_ref().is_some_and(|p| p.0 == lparam.0 as usize) {
                    pending.take().map(|p| p.1)
                } else {
                    None
                }
            });
            if let Some(themes) = themes {
                execute(hwnd, wparam.0, &themes);
            }
            LRESULT(0)
        }
        WM_CONFIG_CHANGED => {
            update_tooltip(hwnd);
            LRESULT(0)
        }
        // Explorer restarted: its tray is empty again.
        _ if msg == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            add_icon(hwnd);
            LRESULT(0)
        }
        // SAFETY: default handling of a message we were called with.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: ICON_ID,
        ..Default::default()
    }
}

/// Copies `text` into the fixed-size tooltip buffer, truncating if needed.
fn set_tip(nid: &mut NOTIFYICONDATAW, text: &str) {
    let max = nid.szTip.len() - 1;
    nid.szTip = [0; 128];
    for (slot, unit) in nid.szTip[..max].iter_mut().zip(text.encode_utf16()) {
        *slot = unit;
    }
}

fn add_icon(hwnd: HWND) {
    let mut nid = icon_data(hwnd);
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = HICON(ICON.load(Ordering::Relaxed) as _);
    set_tip(&mut nid, &tooltip());
    // SAFETY: `nid` is fully initialized and outlives the calls.
    unsafe {
        if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
            log::warn!("could not add the tray icon (is Explorer running?)");
            return;
        }
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
    }
}

fn update_tooltip(hwnd: HWND) {
    let mut nid = icon_data(hwnd);
    nid.uFlags = NIF_TIP | NIF_SHOWTIP;
    set_tip(&mut nid, &tooltip());
    // SAFETY: `nid` is fully initialized and outlives the call.
    let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

/// "mypaper: <theme name>", falling back to the configured name if the theme is unreadable.
fn tooltip() -> String {
    let config = Config::load().unwrap_or_default();
    let name = mypaper_theme::locate(Some(&config.theme))
        .and_then(LoadedTheme::load)
        .map_or(config.theme, |loaded| loaded.theme.name);
    format!("mypaper: {name}")
}

/// The mypaper logo, every size (16–256 px PNG entries; 16–32 px are a hand-tuned small mark).
/// Rendered from `assets/logo/mark.svg` and `mark-small.svg` by `scripts/render-logo.ps1`.
const LOGO: &[u8] = include_bytes!("../../../assets/logo/mypaper.ico");

/// The logo at the tray's small-icon size for the system DPI.
fn make_icon() -> HICON {
    // SAFETY: no preconditions.
    let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()) }.max(16);
    let icon = ico_image(LOGO, size as u32).and_then(|image| {
        // SAFETY: `image` is one complete icon image (PNG or DIB) from the file; 0x30000 is
        // the icon format version the API asks for.
        unsafe { CreateIconFromResourceEx(image, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR) }
            .ok()
    });
    // SAFETY: loads a shared system icon; no ownership is taken.
    icon.unwrap_or_else(|| unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default())
}

/// The image in an `.ico` file that suits `size` px best: the smallest at least that big, else
/// the biggest.
fn ico_image(ico: &[u8], size: u32) -> Option<&[u8]> {
    let count = u16::from_le_bytes(ico.get(4..6)?.try_into().ok()?);
    let entries: Vec<(u32, &[u8])> = (0..usize::from(count))
        .filter_map(|i| {
            let entry = ico.get(6 + i * 16..6 + (i + 1) * 16)?;
            let side = if entry[0] == 0 {
                256
            } else {
                u32::from(entry[0])
            };
            let len = u32::from_le_bytes(entry[8..12].try_into().ok()?) as usize;
            let offset = u32::from_le_bytes(entry[12..16].try_into().ok()?) as usize;
            Some((side, ico.get(offset..offset.checked_add(len)?)?))
        })
        .collect();
    let fits = entries
        .iter()
        .filter(|(side, _)| *side >= size)
        .min_by_key(|(side, _)| *side);
    fits.or_else(|| entries.iter().max_by_key(|(side, _)| *side))
        .map(|(_, image)| *image)
}

/// What a menu item asks for.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    TogglePause,
    SetTheme(String),
    OpenStudio,
    RestartWallpaper,
    EditWidgets,
    ToggleOverlay,
    ToggleTopBar,
    ToggleDock,
    SetTaskbar(TaskbarMode),
    /// Show or hide the desktop icons.
    ToggleDesktopIcons,
    SetIconSize(IconSize),
    /// mypaper's own desktop right-click menu on or off (`desktop_menu.enabled`).
    ToggleDesktopMenu,
    /// The Win11 classic context menu on or off (`classic_context_menu`).
    ToggleClassicMenu,
    Quit,
}

/// Maps a chosen menu id back to its command; `themes` is the submenu's list.
fn decode(id: usize, themes: &[String]) -> Option<Command> {
    match id {
        ID_PAUSE => Some(Command::TogglePause),
        ID_STUDIO => Some(Command::OpenStudio),
        ID_RESTART => Some(Command::RestartWallpaper),
        ID_QUIT => Some(Command::Quit),
        ID_EDIT_WIDGETS => Some(Command::EditWidgets),
        ID_OVERLAY_WIDGETS => Some(Command::ToggleOverlay),
        ID_TOP_BAR => Some(Command::ToggleTopBar),
        ID_DOCK => Some(Command::ToggleDock),
        ID_TASKBAR_KEEP => Some(Command::SetTaskbar(TaskbarMode::Keep)),
        ID_TASKBAR_AUTOHIDE => Some(Command::SetTaskbar(TaskbarMode::Autohide)),
        ID_TASKBAR_HIDE => Some(Command::SetTaskbar(TaskbarMode::Hide)),
        ID_DESKTOP_ICONS => Some(Command::ToggleDesktopIcons),
        ID_ICONS_LARGE => Some(Command::SetIconSize(IconSize::Large)),
        ID_ICONS_MEDIUM => Some(Command::SetIconSize(IconSize::Medium)),
        ID_ICONS_SMALL => Some(Command::SetIconSize(IconSize::Small)),
        ID_DESKTOP_MENU => Some(Command::ToggleDesktopMenu),
        ID_CLASSIC_MENU => Some(Command::ToggleClassicMenu),
        id if id >= ID_THEME => themes.get(id - ID_THEME).cloned().map(Command::SetTheme),
        _ => None,
    }
}

fn show_menu(hwnd: HWND, x: i32, y: i32) {
    PENDING_MENU.with_borrow_mut(|pending| *pending = None);
    let config = Config::load().unwrap_or_else(|e| {
        log::warn!("{e:#}");
        Config::default()
    });
    let themes = list_themes(&mypaper_theme::theme_roots());
    let studio = std::env::current_exe()
        .ok()
        .and_then(|exe| find_studio(&exe));
    let bars = mypaper_bars::config::path()
        .and_then(|path| BarsConfig::load(&path))
        .unwrap_or_else(|e| {
            log::warn!("{e:#}");
            BarsConfig::default()
        });

    // SAFETY: menus are created, shown and destroyed within this call, on our thread.
    let chosen = unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let (Ok(submenu), Ok(bars_menu), Ok(taskbar_menu), Ok(desktop_menu)) = (
            CreatePopupMenu(),
            CreatePopupMenu(),
            CreatePopupMenu(),
            CreatePopupMenu(),
        ) else {
            let _ = DestroyMenu(menu);
            return;
        };
        let check = |on: bool| if on { MF_CHECKED } else { MENU_ITEM_FLAGS(0) };
        append(
            bars_menu,
            MF_STRING | check(bars.top.enabled),
            ID_TOP_BAR,
            "Top bar",
        );
        append(
            bars_menu,
            MF_STRING | check(bars.dock.enabled),
            ID_DOCK,
            "Dock",
        );
        for (id, mode, label) in [
            (ID_TASKBAR_KEEP, TaskbarMode::Keep, "Keep"),
            (ID_TASKBAR_AUTOHIDE, TaskbarMode::Autohide, "Auto-hide"),
            (ID_TASKBAR_HIDE, TaskbarMode::Hide, "Hide"),
        ] {
            let flags = MF_STRING | check(bars.taskbar.mode == mode);
            append(taskbar_menu, flags, id, label);
        }
        append(bars_menu, MF_SEPARATOR, 0, "");
        append(bars_menu, MF_POPUP, taskbar_menu.0 as usize, "Taskbar");
        for (i, name) in themes.iter().enumerate() {
            let checked = if is_active(&config.theme, name) {
                MF_CHECKED
            } else {
                MENU_ITEM_FLAGS(0)
            };
            append(submenu, MF_STRING | checked, ID_THEME + i, name);
        }
        let pause = if config.paused { "Resume" } else { "Pause" };
        append(menu, MF_STRING, ID_PAUSE, pause);
        let themes_flags = if themes.is_empty() {
            MF_GRAYED
        } else {
            MENU_ITEM_FLAGS(0)
        };
        append(menu, MF_POPUP | themes_flags, submenu.0 as usize, "Themes");
        let studio_flags = if studio.is_some() {
            MENU_ITEM_FLAGS(0)
        } else {
            MF_GRAYED
        };
        append(menu, MF_STRING, ID_EDIT_WIDGETS, "Edit widgets");
        append(menu, MF_STRING, ID_OVERLAY_WIDGETS, "Show overlay widgets");
        append(menu, MF_POPUP, bars_menu.0 as usize, "Bars");
        // Desktop: what Explorer's own View menu offers, and mypaper's menu switches. The state
        // comes from Explorer over COM (with a timeout); without it the marks are left off.
        let desktop = desktop_extras::desktop_state();
        let size_mark = |size| check(desktop.is_some_and(|(current, _)| current == size));
        append(
            desktop_menu,
            MF_STRING | check(desktop.is_none_or(|(_, visible)| visible)),
            ID_DESKTOP_ICONS,
            "Show desktop icons",
        );
        append(desktop_menu, MF_SEPARATOR, 0, "");
        for (id, size, label) in [
            (ID_ICONS_LARGE, IconSize::Large, "Large icons"),
            (ID_ICONS_MEDIUM, IconSize::Medium, "Medium icons"),
            (ID_ICONS_SMALL, IconSize::Small, "Small icons"),
        ] {
            append(desktop_menu, MF_STRING | size_mark(size), id, label);
        }
        append(desktop_menu, MF_SEPARATOR, 0, "");
        append(
            desktop_menu,
            MF_STRING | check(config.desktop_menu.enabled),
            ID_DESKTOP_MENU,
            "mypaper right-click menu",
        );
        append(
            desktop_menu,
            MF_STRING | check(config.classic_context_menu),
            ID_CLASSIC_MENU,
            "Classic Windows 11 menu",
        );
        append(menu, MF_POPUP, desktop_menu.0 as usize, "Desktop");
        append(menu, MF_STRING | studio_flags, ID_STUDIO, "Open Studio");
        append(menu, MF_STRING, ID_RESTART, "Restart wallpaper");
        append(menu, MF_SEPARATOR, 0, "");
        append(menu, MF_STRING, ID_QUIT, "Quit");

        if show_shell_menu(hwnd, menu, (x, y), &themes) {
            let _ = DestroyMenu(menu);
            return;
        }

        // Required so the menu closes when the user clicks elsewhere. This
        // only runs on the user's own right-click, so it steals nothing.
        let _ = SetForegroundWindow(hwnd);
        let id = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            x,
            y,
            None,
            hwnd,
            None,
        );
        // Documented companion to SetForegroundWindow + TrackPopupMenu.
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu); // also destroys the submenu
        id.0 as usize
    };

    execute(hwnd, chosen, &themes);
}

fn execute(hwnd: HWND, chosen: usize, themes: &[String]) {
    let bars = mypaper_bars::config::path()
        .and_then(|p| BarsConfig::load(&p))
        .unwrap_or_default();
    let studio = std::env::current_exe()
        .ok()
        .and_then(|exe| find_studio(&exe));
    match decode(chosen, themes) {
        Some(Command::TogglePause) => update_config(hwnd, |c| c.paused = !c.paused),
        Some(Command::SetTheme(name)) => update_config(hwnd, |c| c.theme = name),
        Some(Command::OpenStudio) => {
            if let Some(path) = studio
                && let Err(e) = Process::new(&path).spawn()
            {
                log::error!("could not start {}: {e}", path.display());
            }
        }
        Some(Command::RestartWallpaper) => {
            // SAFETY: posting to the supervisor loop's thread, which is this one.
            let _ = unsafe {
                PostThreadMessageW(
                    GetCurrentThreadId(),
                    WM_RESTART_WALLPAPER,
                    WPARAM(0),
                    LPARAM(0),
                )
            };
        }
        Some(Command::EditWidgets) => toggle_widget_edit_mode(),
        Some(Command::ToggleOverlay) => toggle_overlay_widgets(),
        Some(Command::ToggleTopBar) => edit_bars("top", "enabled", !bars.top.enabled),
        Some(Command::ToggleDock) => edit_bars("dock", "enabled", !bars.dock.enabled),
        Some(Command::SetTaskbar(mode)) => edit_bars("taskbar", "mode", mode),
        Some(Command::ToggleDesktopIcons) => {
            // The icons are shown unless Explorer says otherwise.
            let visible = desktop_extras::desktop_state().is_none_or(|(_, visible)| visible);
            desktop_extras::set_icons_visible(!visible);
        }
        Some(Command::SetIconSize(size)) => desktop_extras::set_icon_size(size),
        Some(Command::ToggleDesktopMenu) => {
            update_config(hwnd, |c| c.desktop_menu.enabled = !c.desktop_menu.enabled);
        }
        Some(Command::ToggleClassicMenu) => {
            update_config(hwnd, |c| c.classic_context_menu = !c.classic_context_menu);
        }
        // SAFETY: posts WM_QUIT to this thread's queue; the supervisor loop cleans up.
        Some(Command::Quit) => unsafe { PostQuitMessage(0) },
        None => {}
    }
}

/// Toggles the widget host's edit mode by posting its registered
/// `mypaper.widgets.edit` message (wParam 2 = toggle) to its message-only
/// window (see docs/widgets.md).
pub(crate) fn toggle_widget_edit_mode() {
    post_to_widget_host(w!("mypaper.widgets.edit"), "edit mode");
}

/// Shows or hides the overlay widgets (the `mypaper.widgets.overlay` message,
/// wParam 2 = toggle; docs/widgets.md).
fn toggle_overlay_widgets() {
    post_to_widget_host(w!("mypaper.widgets.overlay"), "overlay widgets");
}

/// Posts a registered "toggle" message (wParam 2) to the widget host's message-only window.
fn post_to_widget_host(name: PCWSTR, what: &str) {
    // SAFETY: plain lookups and a post; a missing host is just logged.
    unsafe {
        let message = RegisterWindowMessageW(name);
        match FindWindowExW(
            Some(HWND_MESSAGE),
            None,
            w!("mypaper.widgets.host"),
            PCWSTR::null(),
        ) {
            Ok(host) if message != 0 => {
                let _ = PostMessageW(Some(host), message, WPARAM(2), LPARAM(0));
            }
            _ => log::warn!("widget host is not running; cannot toggle {what}"),
        }
    }
}

/// Sets one field of `bars.json` (the bars apply it live). Like the config, a file that doesn't
/// parse is left alone.
fn edit_bars(section: &str, key: &str, value: impl serde::Serialize) {
    let result = mypaper_bars::config::path()
        .and_then(|path| mypaper_bars::config::set_field(&path, section, key, value));
    if let Err(e) = result {
        log::error!("could not update bars.json: {e:#}");
    }
}

fn append(menu: HMENU, flags: MENU_ITEM_FLAGS, id: usize, text: &str) {
    // SAFETY: valid menu handle; the string is copied by the call.
    let _ = unsafe { AppendMenuW(menu, flags, id, &HSTRING::from(text)) };
}

/// Edits `config.json` (the wallpaper picks it up live). Refuses to touch a
/// file that does not parse, so a typo is never overwritten with defaults.
fn update_config(hwnd: HWND, edit: impl FnOnce(&mut Config)) {
    let result = Config::load().and_then(|mut config| {
        edit(&mut config);
        config.save()
    });
    match result {
        Ok(()) => update_tooltip(hwnd),
        Err(e) => log::error!("could not update the config: {e:#}"),
    }
}

/// Is the config's theme (a name or a folder path) the theme folder called `name`?
fn is_active(config_theme: &str, name: &str) -> bool {
    Path::new(config_theme)
        .file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
}

/// Theme folder names across `roots`, sorted. A name in several roots is
/// listed once, matching `mypaper_theme::locate`, which picks the first.
fn list_themes(roots: &[PathBuf]) -> Vec<String> {
    roots
        .iter()
        .filter_map(|root| std::fs::read_dir(root).ok())
        .flatten()
        .flatten()
        .filter(|entry| entry.path().join(mypaper_theme::MANIFEST).is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// The studio app: beside this exe, or in a dev build of `apps/studio`
/// under this exe's folder or any parent.
fn find_studio(exe: &Path) -> Option<PathBuf> {
    let beside = exe.with_file_name(STUDIO_EXE);
    let dev = exe.ancestors().skip(1).flat_map(|dir| {
        ["release", "debug"].map(|profile| {
            dir.join("apps/studio/src-tauri/target")
                .join(profile)
                .join(STUDIO_EXE)
        })
    });
    std::iter::once(beside)
        .chain(dev)
        .find(|path| path.is_file())
}

/// Copies the stock menu model; the shell presents the same command ids and checks.
fn menu_items(menu: HMENU) -> Vec<wire::Item> {
    use windows::Win32::UI::WindowsAndMessaging::*;
    // SAFETY: the menu and all submenus remain alive throughout this read-only traversal.
    unsafe {
        (0..GetMenuItemCount(Some(menu)))
            .filter_map(|i| {
                let flags = GetMenuState(menu, i as u32, MF_BYPOSITION);
                if flags == u32::MAX {
                    return None;
                }
                if flags & MF_SEPARATOR.0 != 0 {
                    return Some(wire::Item::Separator);
                }
                let mut text = [0u16; 512];
                let len = GetMenuStringW(menu, i as u32, Some(&mut text), MF_BYPOSITION);
                let label = String::from_utf16_lossy(&text[..len.max(0) as usize]);
                let submenu = GetSubMenu(menu, i);
                Some(
                    if !submenu.is_invalid() && flags & (MF_GRAYED.0 | MF_DISABLED.0) == 0 {
                        wire::Item::Submenu {
                            label,
                            items: menu_items(submenu),
                        }
                    } else {
                        wire::Item::Action {
                            label,
                            id: GetMenuItemID(menu, i),
                            checked: flags & MF_CHECKED.0 != 0,
                            enabled: flags & (MF_GRAYED.0 | MF_DISABLED.0) == 0,
                        }
                    },
                )
            })
            .collect()
    }
}

/// Core sends a copied model and returns immediately; only an unavailable shell uses Win32.
fn show_shell_menu(caller: HWND, menu: HMENU, point: (i32, i32), themes: &[String]) -> bool {
    use windows::Win32::{System::DataExchange::COPYDATASTRUCT, UI::WindowsAndMessaging::*};
    // SAFETY: look up our dedicated message-only inbox, then send owned bytes with a bounded wait.
    unsafe {
        let Ok(host) = FindWindowExW(Some(HWND_MESSAGE), None, wire::HOST, PCWSTR::null()) else {
            return false;
        };
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let request = wire::Request {
            nonce,
            point,
            items: menu_items(menu),
        };
        let Ok(bytes) = serde_json::to_vec(&request) else {
            return false;
        };
        let packet = COPYDATASTRUCT {
            dwData: wire::MAGIC,
            cbData: bytes.len() as u32,
            lpData: bytes.as_ptr() as _,
        };
        let mut result = 0usize;
        let sent = SendMessageTimeoutW(
            host,
            WM_COPYDATA,
            WPARAM(caller.0 as usize),
            LPARAM(&packet as *const _ as isize),
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            20,
            Some(&mut result),
        );
        if sent.0 == 0 || result != 1 {
            return false;
        }
        PENDING_MENU.with_borrow_mut(|p| *p = Some((nonce, themes.to_vec())));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The production tray loop for the exclusive-desktop IPC/fallback performance harness.
    #[test]
    #[ignore = "takes over the tray; launch only under IMPLEMENTING.md's desktop lock"]
    fn live_tray_fixture() {
        mypaper_desktop::init_logging();
        mypaper_desktop::enable_dpi_awareness();
        let _tray = Tray::new().unwrap();
        let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
        // SAFETY: the fixture's window and ordinary message loop live on this thread.
        unsafe {
            while windows::Win32::UI::WindowsAndMessaging::GetMessageW(&mut msg, None, 0, 0)
                .as_bool()
            {
                let _ = windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
                windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
            }
        }
    }

    #[test]
    fn tray_snapshot_preserves_disabled_commands_separators_checks_and_submenus() {
        // SAFETY: these hidden menu handles are created/read/destroyed on this test thread.
        unsafe {
            let menu = CreatePopupMenu().unwrap();
            let sub = CreatePopupMenu().unwrap();
            append(menu, MF_STRING | MF_CHECKED, ID_PAUSE, "Pause");
            append(menu, MF_SEPARATOR, 0, "");
            append(sub, MF_STRING | MF_GRAYED, ID_STUDIO, "Studio");
            append(menu, MF_POPUP, sub.0 as usize, "Tools");
            let items = menu_items(menu);
            assert!(
                matches!(&items[0], wire::Item::Action { id, checked: true, enabled: true, .. } if *id == ID_PAUSE as u32)
            );
            assert!(matches!(items[1], wire::Item::Separator));
            let wire::Item::Submenu { items, .. } = &items[2] else {
                panic!("submenu")
            };
            assert!(matches!(
                items[0],
                wire::Item::Action { enabled: false, .. }
            ));
            let _ = DestroyMenu(menu);
        }
    }

    /// A scratch folder removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("mypaper-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn touch(&self, relative: &str) {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn lists_theme_folders_once_and_skips_other_folders() {
        let user = Scratch::new("themes-user");
        let bundled = Scratch::new("themes-bundled");
        user.touch("mine/theme.json");
        user.touch("aurora/theme.json");
        bundled.touch("aurora/theme.json");
        bundled.touch("zen/theme.json");
        bundled.touch("not-a-theme/readme.txt");
        let roots = [user.0.clone(), bundled.0.clone(), user.0.join("missing")];
        assert_eq!(list_themes(&roots), ["aurora", "mine", "zen"]);
    }

    #[test]
    fn the_logo_has_an_image_for_every_tray_size() {
        // An entry is a PNG (its width: big-endian at byte 16, in IHDR) or a DIB (biWidth:
        // little-endian at byte 4).
        let side = |size| {
            let image = ico_image(LOGO, size).expect("an image");
            if image.starts_with(b"\x89PNG") {
                u32::from_be_bytes(image[16..20].try_into().unwrap())
            } else {
                u32::from_le_bytes(image[4..8].try_into().unwrap())
            }
        };
        assert_eq!(side(16), 16);
        assert!(side(20) >= 20, "the next size up");
        assert_eq!(side(1000), 256, "too big: the biggest there is");
        assert_eq!(ico_image(b"\0\0\x01\0\x01\0", 16), None, "a truncated file");
    }

    #[test]
    fn active_theme_matches_name_or_folder_path() {
        assert!(is_active("aurora", "aurora"));
        assert!(is_active("Aurora", "aurora"));
        assert!(is_active(r"D:\themes\aurora", "aurora"));
        assert!(!is_active("aurora", "zen"));
    }

    #[test]
    fn menu_ids_decode_to_commands() {
        let themes = ["aurora".to_owned(), "zen".to_owned()];
        assert_eq!(decode(ID_PAUSE, &themes), Some(Command::TogglePause));
        assert_eq!(decode(ID_QUIT, &themes), Some(Command::Quit));
        assert_eq!(decode(ID_EDIT_WIDGETS, &themes), Some(Command::EditWidgets));
        assert_eq!(
            decode(ID_OVERLAY_WIDGETS, &themes),
            Some(Command::ToggleOverlay)
        );
        assert_eq!(decode(ID_DOCK, &themes), Some(Command::ToggleDock));
        assert_eq!(
            decode(ID_TASKBAR_HIDE, &themes),
            Some(Command::SetTaskbar(TaskbarMode::Hide))
        );
        assert_eq!(
            decode(ID_THEME + 1, &themes),
            Some(Command::SetTheme("zen".into()))
        );
        assert_eq!(decode(ID_THEME + 2, &themes), None);
        assert_eq!(decode(0, &themes), None); // menu dismissed
    }

    #[test]
    fn desktop_menu_ids_decode_and_stay_clear_of_the_theme_range() {
        let none: [String; 0] = [];
        assert_eq!(
            decode(ID_DESKTOP_ICONS, &none),
            Some(Command::ToggleDesktopIcons)
        );
        assert_eq!(
            decode(ID_ICONS_LARGE, &none),
            Some(Command::SetIconSize(IconSize::Large))
        );
        assert_eq!(
            decode(ID_ICONS_MEDIUM, &none),
            Some(Command::SetIconSize(IconSize::Medium))
        );
        assert_eq!(
            decode(ID_ICONS_SMALL, &none),
            Some(Command::SetIconSize(IconSize::Small))
        );
        assert_eq!(
            decode(ID_DESKTOP_MENU, &none),
            Some(Command::ToggleDesktopMenu)
        );
        assert_eq!(
            decode(ID_CLASSIC_MENU, &none),
            Some(Command::ToggleClassicMenu)
        );
        // Themes start at ID_THEME; no fixed id may reach it.
        const { assert!(ID_CLASSIC_MENU < ID_THEME) };
    }

    #[test]
    fn finds_studio_beside_exe_or_in_a_dev_build() {
        let root = Scratch::new("studio");
        let exe = root.0.join("target/debug/mypaper-core.exe");
        assert_eq!(find_studio(&exe), None);

        root.touch("apps/studio/src-tauri/target/debug/mypaper-studio.exe");
        assert_eq!(
            find_studio(&exe),
            Some(
                root.0
                    .join("apps/studio/src-tauri/target/debug/mypaper-studio.exe")
            )
        );

        root.touch("target/debug/mypaper-studio.exe");
        assert_eq!(
            find_studio(&exe),
            Some(root.0.join("target/debug/mypaper-studio.exe"))
        );
    }
}
