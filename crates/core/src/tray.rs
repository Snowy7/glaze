//! The tray icon and its right-click menu: pause, theme picker, studio,
//! restart and quit. Everything goes through `config.json` (the wallpaper
//! applies it live) or a message to the supervisor loop, so the tray holds
//! no state of its own.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use anyhow::{Context, Result};
use mypaper_config::Config;
use mypaper_desktop::{ConfigWatcher, watch_config};
use mypaper_theme::LoadedTheme;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIcon, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon,
    DestroyMenu, DestroyWindow, HICON, HMENU, IDI_APPLICATION, LoadIconW, MENU_ITEM_FLAGS,
    MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, PostMessageW, PostQuitMessage,
    PostThreadMessageW, RegisterClassExW, RegisterWindowMessageW, SetForegroundWindow,
    TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_CONTEXTMENU,
    WM_NULL, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{HSTRING, PCWSTR, w};

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
/// Theme `i` in the Themes submenu has id `ID_THEME + i`.
const ID_THEME: usize = 100;

/// Set once at startup; the window procedure needs them for re-adding the icon.
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

/// A round gradient badge, drawn in code so no icon asset has to ship.
fn make_icon() -> HICON {
    const N: usize = 32;
    let mut colors = [0u8; N * N * 4]; // BGRA
    let mut mask = [0xFFu8; N * N / 8]; // 1 bits are transparent
    let radius = N as f32 / 2.0;
    for y in 0..N {
        for x in 0..N {
            let (dx, dy) = (x as f32 + 0.5 - radius, y as f32 + 0.5 - radius);
            if dx * dx + dy * dy > (radius - 1.0).powi(2) {
                continue;
            }
            let t = y as f32 / (N - 1) as f32;
            let mix = |from: f32, to: f32| (from + (to - from) * t) as u8;
            // Teal to violet, top to bottom.
            let pixel = [mix(191.0, 237.0), mix(212.0, 58.0), mix(45.0, 124.0), 255];
            colors[(y * N + x) * 4..][..4].copy_from_slice(&pixel);
            mask[y * (N / 8) + x / 8] &= !(0x80 >> (x % 8));
        }
    }
    // SAFETY: both buffers match the stated 32x32 size and formats.
    let icon = unsafe {
        let hinstance = GetModuleHandleW(None).ok().map(Into::into);
        CreateIcon(
            hinstance,
            N as i32,
            N as i32,
            1,
            32,
            mask.as_ptr(),
            colors.as_ptr(),
        )
    };
    // SAFETY: loads a shared system icon; no ownership is taken.
    icon.unwrap_or_else(|_| unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default())
}

/// What a menu item asks for.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    TogglePause,
    SetTheme(String),
    OpenStudio,
    RestartWallpaper,
    Quit,
}

/// Maps a chosen menu id back to its command; `themes` is the submenu's list.
fn decode(id: usize, themes: &[String]) -> Option<Command> {
    match id {
        ID_PAUSE => Some(Command::TogglePause),
        ID_STUDIO => Some(Command::OpenStudio),
        ID_RESTART => Some(Command::RestartWallpaper),
        ID_QUIT => Some(Command::Quit),
        id if id >= ID_THEME => themes.get(id - ID_THEME).cloned().map(Command::SetTheme),
        _ => None,
    }
}

fn show_menu(hwnd: HWND, x: i32, y: i32) {
    let config = Config::load().unwrap_or_else(|e| {
        log::warn!("{e:#}");
        Config::default()
    });
    let themes = list_themes(&mypaper_theme::theme_roots());
    let studio = std::env::current_exe()
        .ok()
        .and_then(|exe| find_studio(&exe));

    // SAFETY: menus are created, shown and destroyed within this call, on our thread.
    let chosen = unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let Ok(submenu) = CreatePopupMenu() else {
            let _ = DestroyMenu(menu);
            return;
        };
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
        append(menu, MF_STRING | studio_flags, ID_STUDIO, "Open Studio");
        append(menu, MF_STRING, ID_RESTART, "Restart wallpaper");
        append(menu, MF_SEPARATOR, 0, "");
        append(menu, MF_STRING, ID_QUIT, "Quit");

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

    match decode(chosen, &themes) {
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
        // SAFETY: posts WM_QUIT to this thread's queue; the supervisor loop cleans up.
        Some(Command::Quit) => unsafe { PostQuitMessage(0) },
        None => {}
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            decode(ID_THEME + 1, &themes),
            Some(Command::SetTheme("zen".into()))
        );
        assert_eq!(decode(ID_THEME + 2, &themes), None);
        assert_eq!(decode(0, &themes), None); // menu dismissed
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
