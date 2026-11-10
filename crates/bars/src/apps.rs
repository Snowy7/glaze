//! The windows a taskbar would show, grouped into apps, and the dock's items built from them and
//! the pins (docs/bars.md, "Dock").
//!
//! The rules (eligibility, grouping, ordering) are pure functions; [`Apps`] reads the real
//! windows when the hooks say something changed.

use std::collections::HashMap;

use serde::Serialize;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromWindow};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GA_ROOTOWNER, GW_OWNER, GWL_EXSTYLE, GetAncestor, GetClassNameW, GetWindow,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, PWSTR};

/// What decides whether a taskbar shows a top-level window.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub visible: bool,
    /// Cloaked by DWM: on another virtual desktop, or a suspended UWP app.
    pub cloaked: bool,
    pub owned: bool,
    pub tool: bool,
    pub app_window: bool,
    pub no_activate: bool,
    /// One of the bars' own windows.
    pub ours: bool,
    pub class: String,
}

/// Classes that are never apps: the desktop, the taskbars, and UWP's inner core windows (their
/// apps show through `ApplicationFrameWindow`).
const NOT_APPS: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Windows.UI.Core.CoreWindow",
];

/// The taskbar's rule: visible, not cloaked, and either `WS_EX_APPWINDOW` or unowned, not a tool
/// window and activatable.
pub fn taskbar_eligible(facts: &Facts) -> bool {
    facts.visible
        && !facts.cloaked
        && !facts.ours
        && !NOT_APPS.contains(&facts.class.as_str())
        && (facts.app_window || (!facts.owned && !facts.tool && !facts.no_activate))
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppWindow {
    pub hwnd: isize,
    pub title: String,
    pub minimized: bool,
    /// The window's `PKEY_AppUserModel_ID`.
    pub aumid: Option<String>,
    /// The process image, lowercase.
    pub exe: Option<String>,
    /// The monitor it's on (`HMONITOR`; where it will restore to, if minimized).
    pub monitor: isize,
}

/// One app's windows: the same AppUserModelID, or the same process image when there is none.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub key: String,
    pub aumid: Option<String>,
    pub exe: Option<String>,
    /// Most recent (highest in the z-order) first.
    pub windows: Vec<AppWindow>,
}

pub fn group_key(window: &AppWindow) -> String {
    match (&window.aumid, &window.exe) {
        (Some(aumid), _) => format!("app:{}", aumid.to_lowercase()),
        (None, Some(exe)) => format!("exe:{}", exe_identity(exe)),
        (None, None) => format!("window:{}", window.hwnd),
    }
}

/// What identifies an app's executable across its updates: the path, lowercase, with a
/// Squirrel version folder folded away (`…\Discord\app-1.0.9188\Discord.exe` is
/// `…\discord\discord.exe`), which is also what its shortcut starts ([`squirrel_target`]).
pub fn exe_identity(exe: &str) -> String {
    let exe = exe.to_lowercase().replace('/', "\\");
    let parts: Vec<&str> = exe.rsplitn(3, '\\').collect();
    match parts.as_slice() {
        [file, version, root] if version.starts_with("app-") => format!("{root}\\{file}"),
        _ => exe,
    }
}

/// The app a Squirrel shortcut starts (Discord, Slack, Teams classic): its target is
/// `…\Update.exe` with `--processStart "Discord.exe"`, which runs `…\app-<version>\Discord.exe`.
/// Returns that app's [`exe_identity`] (`…\discord.exe`), or `None` for any other shortcut.
pub fn squirrel_target(target: &str, args: &str) -> Option<String> {
    let (dir, file) = target.rsplit_once(['\\', '/'])?;
    if !file.eq_ignore_ascii_case("update.exe") {
        return None;
    }
    let lower = args.to_lowercase();
    let at = lower.find("--processstart")? + "--processstart".len();
    let rest = args[at..].trim_start_matches(['=', ' ']);
    let name = match rest.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next()?,
        None => rest.split_whitespace().next()?,
    };
    (!name.is_empty()).then(|| format!("{dir}\\{name}").to_lowercase())
}

/// The AUMID Squirrel registers for an app installed in `…\<Name>\` (Discord, Slack, Teams
/// classic): `com.squirrel.<name>.<name>`, lowercase. Its toasts carry it even while the app
/// runs only in the tray, with no window to read an AUMID from.
pub fn squirrel_aumid(exe: &str) -> Option<String> {
    let identity = exe_identity(exe);
    let mut dirs = identity.rsplit('\\').skip(1);
    let name = match dirs.next()? {
        // Teams classic runs from `…\Microsoft\Teams\current\Teams.exe`.
        "current" => dirs.next()?,
        name => name,
    };
    (!name.is_empty() && !name.contains(':')).then(|| format!("com.squirrel.{name}.{name}"))
}

/// Groups windows given in z-order (top first); groups come in the order of their top window.
pub fn group(windows: Vec<AppWindow>) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for window in windows {
        let key = group_key(&window);
        match groups.iter_mut().find(|g| g.key == key) {
            Some(group) => group.windows.push(window),
            None => groups.push(Group {
                key,
                aumid: window.aumid.clone(),
                exe: window.exe.clone(),
                windows: vec![window],
            }),
        }
    }
    groups
}

/// Keeps the first-seen order of running apps, so focusing a window never reshuffles the dock:
/// apps that quit leave, new ones join at the end.
pub fn update_order(order: &mut Vec<String>, groups: &[Group]) {
    order.retain(|key| groups.iter().any(|g| g.key == *key));
    for group in groups {
        if !order.contains(&group.key) {
            order.push(group.key.clone());
        }
    }
}

/// A pinned item, with what identifies its app once resolved (a shortcut's target and AUMID).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pin {
    pub path: String,
    pub name: String,
    pub icon: Option<String>,
    pub aumid: Option<String>,
    /// Lowercase.
    pub exe: Option<String>,
}

impl Pin {
    /// The same AppUserModelID, else the same executable across updates ([`exe_identity`]).
    fn matches(&self, group: &Group) -> bool {
        let same = |a: &Option<String>, b: &Option<String>| matches!((a, b), (Some(a), Some(b)) if a.eq_ignore_ascii_case(b));
        let exe = |e: &Option<String>| e.as_deref().map(exe_identity);
        match (&self.aumid, &group.aumid) {
            (Some(_), Some(_)) => same(&self.aumid, &group.aumid),
            _ => same(&exe(&self.exe), &exe(&group.exe)),
        }
    }
}

/// How a running app looks in the dock, resolved by the caller.
#[derive(Debug, Clone, PartialEq)]
pub struct Running {
    pub group: Group,
    pub name: String,
    /// What launches it again (`shell:AppsFolder\…` or its exe).
    pub path: String,
    pub icon: Option<String>,
}

/// A dock item, as the page gets it (`apps` event).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub key: String,
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub pinned: bool,
    pub windows: Vec<ItemWindow>,
    /// What identifies the app, for icon packs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aumid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exe: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ItemWindow {
    pub hwnd: isize,
    pub title: String,
    pub active: bool,
    pub minimized: bool,
    /// Flashing for attention (`HSHELL_FLASH`) since it was last in front.
    pub attention: bool,
}

/// The key of a pinned item: its path, case-insensitively.
pub fn pin_key(path: &str) -> String {
    format!("pin:{}", path.to_lowercase())
}

/// What one dock shows of the running apps.
#[derive(Debug, Clone, Copy, Default)]
pub struct View<'a> {
    /// Running apps that aren't pinned.
    pub running: crate::config::Running,
    /// The dock's monitor, for `running: "monitor"`.
    pub monitor: isize,
    /// The window in front.
    pub active: Option<isize>,
    /// Windows asking for attention.
    pub attention: Option<&'a std::collections::HashSet<isize>>,
}

/// The dock: pinned items in order (with the windows of every app they match), then the other
/// running apps in `order`, as `view` says.
pub fn dock_items(pins: &[Pin], running: &[Running], order: &[String], view: View) -> Vec<Item> {
    use crate::config::Running as Show;
    let here = |w: &AppWindow| view.running != Show::Monitor || w.monitor == view.monitor;
    let windows_of = |groups: &[&Group]| -> Vec<ItemWindow> {
        groups
            .iter()
            .flat_map(|g| &g.windows)
            .filter(|w| here(w))
            .map(|w| ItemWindow {
                hwnd: w.hwnd,
                title: w.title.clone(),
                active: Some(w.hwnd) == view.active,
                minimized: w.minimized,
                attention: view.attention.is_some_and(|a| a.contains(&w.hwnd)),
            })
            .collect()
    };
    let mut claimed = vec![false; running.len()];
    let mut items: Vec<Item> = pins
        .iter()
        .map(|pin| {
            let groups: Vec<&Group> = running
                .iter()
                .zip(claimed.iter_mut())
                .filter(|(app, claimed)| !**claimed && pin.matches(&app.group))
                .map(|(app, claimed)| {
                    *claimed = true;
                    &app.group
                })
                .collect();
            Item {
                key: pin_key(&pin.path),
                name: pin.name.clone(),
                path: pin.path.clone(),
                icon: pin.icon.clone(),
                pinned: true,
                windows: windows_of(&groups),
                // A Squirrel pin (`Update.exe --processStart Discord.exe`) has no AUMID of its
                // own; its windows' is what notifications and badges key by.
                aumid: pin
                    .aumid
                    .clone()
                    .or_else(|| groups.iter().find_map(|g| g.aumid.clone())),
                exe: pin
                    .exe
                    .clone()
                    .or_else(|| groups.iter().find_map(|g| g.exe.clone())),
            }
        })
        .collect();
    if view.running != Show::None {
        let mut rest: Vec<&Running> = running
            .iter()
            .zip(&claimed)
            .filter(|(app, claimed)| !**claimed && app.group.windows.iter().any(here))
            .map(|(app, _)| app)
            .collect();
        rest.sort_by_key(|app| order.iter().position(|k| *k == app.group.key));
        items.extend(rest.into_iter().map(|app| Item {
            key: app.group.key.clone(),
            name: app.name.clone(),
            path: app.path.clone(),
            icon: app.icon.clone(),
            pinned: false,
            windows: windows_of(&[&app.group]),
            aumid: app.group.aumid.clone(),
            exe: app.group.exe.clone(),
        }));
    }
    items
}

/// A readable name from a path: the file name without its extension.
pub fn file_stem(path: &str) -> String {
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_owned(),
        _ => name.to_owned(),
    }
}

// --- The real windows ------------------------------------------------------------------------------

/// The running apps, kept current by [`refresh`](Apps::refresh).
#[derive(Default)]
pub struct Apps {
    pub groups: Vec<Group>,
    /// First-seen order of the groups (see [`update_order`]).
    pub order: Vec<String>,
    exes: HashMap<u32, Option<String>>,
}

impl Apps {
    /// Re-reads the windows. Returns whether anything the dock shows changed.
    pub fn refresh(&mut self) -> bool {
        let ours = std::process::id();
        let mut seen = HashMap::new();
        let windows: Vec<AppWindow> = top_level_windows()
            .into_iter()
            .filter_map(|hwnd| {
                let (facts, pid) = facts(hwnd, ours);
                taskbar_eligible(&facts).then(|| {
                    let exe = self.exes.remove(&pid).unwrap_or_else(|| process_image(pid));
                    seen.insert(pid, exe.clone());
                    read_window(hwnd, exe)
                })
            })
            .collect();
        self.exes = seen;
        let groups = group(windows);
        update_order(&mut self.order, &groups);
        let changed = groups != self.groups;
        self.groups = groups;
        changed
    }

    /// The group of `hwnd` (or of the window that owns it).
    pub fn group_of(&self, hwnd: isize) -> Option<&Group> {
        let root = root_owner(hwnd);
        self.groups
            .iter()
            .find(|g| g.windows.iter().any(|w| w.hwnd == hwnd || w.hwnd == root))
    }
}

/// The app window highest in the z-order that isn't minimized (nor `except`): where the
/// foreground goes when the bars hand it back.
pub fn front_window(except: Option<isize>) -> Option<isize> {
    let ours = std::process::id();
    top_level_windows()
        .into_iter()
        .filter(|hwnd| Some(hwnd.0 as isize) != except)
        .find(|&hwnd| {
            // SAFETY: a read-only query on a handle from EnumWindows.
            let minimized = unsafe { IsIconic(hwnd) }.as_bool();
            !minimized && taskbar_eligible(&facts(hwnd, ours).0)
        })
        .map(|hwnd| hwnd.0 as isize)
}

/// The top-level window that owns `hwnd` (itself if unowned).
pub fn root_owner(hwnd: isize) -> isize {
    // SAFETY: a read-only query; an invalid handle gives a null result.
    let root = unsafe { GetAncestor(HWND(hwnd as _), GA_ROOTOWNER) };
    if root.is_invalid() {
        hwnd
    } else {
        root.0 as isize
    }
}

fn top_level_windows() -> Vec<HWND> {
    let mut out: Vec<HWND> = Vec::new();
    // SAFETY: the callback only runs during this call, while `out` is alive.
    unsafe {
        let _ = EnumWindows(Some(push_window), LPARAM(&raw mut out as isize));
    }
    out
}

unsafe extern "system" fn push_window(hwnd: HWND, out: LPARAM) -> BOOL {
    // SAFETY: `out` is the Vec passed by `top_level_windows`.
    unsafe { (*(out.0 as *mut Vec<HWND>)).push(hwnd) };
    true.into()
}

pub fn class_name(hwnd: HWND) -> String {
    let mut class = [0u16; 64];
    // SAFETY: the buffer outlives the call.
    let len = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
    String::from_utf16_lossy(&class[..len])
}

/// What [`taskbar_eligible`] needs to know about `hwnd`, and its process. A hidden window (most
/// top-level windows) is answered without the class name or DWM's cloak state (a call into DWM).
fn facts(hwnd: HWND, ours: u32) -> (Facts, u32) {
    // SAFETY: read-only queries on a handle from EnumWindows.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return (Facts::default(), 0);
        }
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&raw mut cloaked).cast(),
            size_of::<u32>() as u32,
        );
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let facts = Facts {
            visible: IsWindowVisible(hwnd).as_bool(),
            cloaked: cloaked != 0,
            owned: GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid()),
            tool: ex & WS_EX_TOOLWINDOW.0 != 0,
            app_window: ex & WS_EX_APPWINDOW.0 != 0,
            no_activate: ex & WS_EX_NOACTIVATE.0 != 0,
            ours: pid == ours,
            class: class_name(hwnd),
        };
        (facts, pid)
    }
}

fn read_window(hwnd: HWND, exe: Option<String>) -> AppWindow {
    AppWindow {
        hwnd: hwnd.0 as isize,
        title: window_title(hwnd),
        // SAFETY: a read-only query.
        minimized: unsafe { IsIconic(hwnd) }.as_bool(),
        aumid: window_aumid(hwnd),
        exe,
        monitor: window_monitor(hwnd),
    }
}

/// The monitor a window is on (for a minimized one, where it restores to).
pub fn window_monitor(hwnd: HWND) -> isize {
    // SAFETY: a read-only query; a stale handle gives the nearest monitor.
    unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }.0 as isize
}

/// The title. For another process's window this reads the stored caption; it never sends that
/// window a message, so a hung app can't block us.
pub fn window_title(hwnd: HWND) -> String {
    // SAFETY: the buffer outlives the call and is as long as we say.
    unsafe {
        let len = GetWindowTextLengthW(hwnd).max(0) as usize;
        let mut buffer = vec![0u16; len + 1];
        let copied = GetWindowTextW(hwnd, &mut buffer).max(0) as usize;
        String::from_utf16_lossy(&buffer[..copied.min(len)])
    }
}

/// The window's AppUserModelID, if the app set one.
fn window_aumid(hwnd: HWND) -> Option<String> {
    // SAFETY: COM calls on a store we own; the string is freed after copying.
    unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd).ok()?;
        let value = store.GetValue(&PKEY_AppUserModel_ID).ok()?;
        let text: PWSTR = PropVariantToStringAlloc(&value).ok()?;
        let id = text.to_string().ok();
        CoTaskMemFree(Some(text.0.cast()));
        id.filter(|id| !id.is_empty())
    }
}

/// The process image path, lowercase. A running exe renamed aside (`deploy-local.ps1` moves an
/// open Studio's to `<name>.exe.old-<stamp>`) is reported at its new name; it reads as the
/// original exe in the same folder, so its icon and identity stay the app's.
fn process_image(pid: u32) -> Option<String> {
    process_image_path(pid).map(|path| renamed_aside(&path).unwrap_or(path))
}

/// `x.exe` for `x.exe.old-<anything>`.
fn renamed_aside(path: &str) -> Option<String> {
    let (exe, _) = path.rsplit_once(".exe.old-")?;
    Some(format!("{exe}.exe"))
}

fn process_image_path(pid: u32) -> Option<String> {
    // SAFETY: a limited query handle, closed before returning; the buffer outlives the call.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        ok.ok()?;
        Some(String::from_utf16_lossy(&buffer[..len as usize]).to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exe_renamed_aside_reads_as_the_original() {
        let dir = r"d:\wallpaper\target\release";
        assert_eq!(
            renamed_aside(&format!(r"{dir}\glaze-studio.exe.old-20261105183342")),
            Some(format!(r"{dir}\glaze-studio.exe"))
        );
        assert_eq!(renamed_aside(&format!(r"{dir}\glaze-studio.exe")), None);
    }

    fn app_window() -> Facts {
        Facts {
            visible: true,
            class: "Chrome_WidgetWin_1".into(),
            ..Facts::default()
        }
    }

    #[test]
    fn eligibility_follows_the_taskbar_rule() {
        assert!(taskbar_eligible(&app_window()));
        let not = |change: fn(&mut Facts)| {
            let mut facts = app_window();
            change(&mut facts);
            !taskbar_eligible(&facts)
        };
        assert!(not(|f| f.visible = false));
        assert!(not(|f| f.cloaked = true), "other desktops, suspended apps");
        assert!(not(|f| f.owned = true), "dialogs");
        assert!(not(|f| f.tool = true));
        assert!(not(|f| f.no_activate = true), "overlays");
        assert!(not(|f| f.ours = true));
        assert!(not(|f| f.class = "Progman".into()));
        assert!(not(|f| f.class = "Shell_SecondaryTrayWnd".into()));
        assert!(not(|f| f.class = "Windows.UI.Core.CoreWindow".into()));
        // WS_EX_APPWINDOW puts owned and tool windows on the taskbar...
        let mut facts = app_window();
        facts.owned = true;
        facts.tool = true;
        facts.app_window = true;
        assert!(taskbar_eligible(&facts));
        // ...but never hidden or cloaked ones.
        facts.cloaked = true;
        assert!(!taskbar_eligible(&facts));
    }

    fn window(hwnd: isize, aumid: Option<&str>, exe: Option<&str>) -> AppWindow {
        AppWindow {
            hwnd,
            title: format!("window {hwnd}"),
            minimized: false,
            aumid: aumid.map(Into::into),
            exe: exe.map(Into::into),
            monitor: 1,
        }
    }

    const EXPLORER: &str = r"c:\windows\explorer.exe";
    const CHROME: &str = r"c:\program files\google\chrome\application\chrome.exe";
    const FRAME_HOST: &str = r"c:\windows\system32\applicationframehost.exe";

    #[test]
    fn groups_by_aumid_then_by_exe() {
        let groups = group(vec![
            window(1, None, Some(CHROME)),
            window(
                2,
                Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"),
                Some(FRAME_HOST),
            ),
            window(3, None, Some(CHROME)),
            window(
                4,
                Some("Microsoft.WindowsStore_8wekyb3d8bbwe!App"),
                Some(FRAME_HOST),
            ),
            window(5, Some("Microsoft.Windows.Explorer"), Some(EXPLORER)),
            window(6, None, None),
        ]);
        let keys: Vec<&str> = groups.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                format!("exe:{CHROME}").as_str(),
                "app:microsoft.windowscalculator_8wekyb3d8bbwe!app",
                "app:microsoft.windowsstore_8wekyb3d8bbwe!app",
                "app:microsoft.windows.explorer",
                "window:6",
            ],
            "UWP apps sharing ApplicationFrameHost stay apart"
        );
        let hwnds: Vec<isize> = groups[0].windows.iter().map(|w| w.hwnd).collect();
        assert_eq!(hwnds, [1, 3], "z-order kept inside a group");
    }

    #[test]
    fn running_order_is_first_seen_and_survives_focus_changes() {
        let groups = |exes: &[&str]| {
            group(
                exes.iter()
                    .enumerate()
                    .map(|(i, exe)| window(i as isize, None, Some(exe)))
                    .collect(),
            )
        };
        let mut order = Vec::new();
        update_order(&mut order, &groups(&["a", "b"]));
        update_order(&mut order, &groups(&["b", "a", "c"])); // b came to the front, c started
        assert_eq!(order, ["exe:a", "exe:b", "exe:c"]);
        update_order(&mut order, &groups(&["c", "b"])); // a quit
        assert_eq!(order, ["exe:b", "exe:c"]);
    }

    fn running(group: Group, name: &str) -> Running {
        Running {
            path: group.exe.clone().unwrap_or_default(),
            name: name.into(),
            icon: None,
            group,
        }
    }

    #[test]
    fn pins_come_first_and_claim_their_running_apps() {
        let groups = group(vec![
            window(10, None, Some(CHROME)),
            window(11, Some("Microsoft.Windows.Explorer"), Some(EXPLORER)),
            window(12, Some("Spotify"), Some(r"c:\spotify.exe")),
            window(13, None, Some(CHROME)),
        ]);
        let mut order = Vec::new();
        update_order(&mut order, &groups);
        let apps: Vec<Running> = groups
            .into_iter()
            .map(|g| {
                let name = file_stem(g.exe.as_deref().unwrap_or_default());
                running(g, &name)
            })
            .collect();
        let pins = [
            // An exe pin matches by image, an AppsFolder pin by AUMID.
            Pin {
                path: r"C:\Windows\explorer.exe".into(),
                name: "File Explorer".into(),
                exe: Some(EXPLORER.into()),
                ..Pin::default()
            },
            Pin {
                path: r"shell:AppsFolder\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App".into(),
                name: "Terminal".into(),
                aumid: Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App".into()),
                ..Pin::default()
            },
        ];
        let flashing = std::collections::HashSet::from([12]);
        let view = View {
            active: Some(13),
            attention: Some(&flashing),
            ..View::default()
        };
        let items = dock_items(&pins, &apps, &order, view);
        let summary: Vec<(&str, bool, usize)> = items
            .iter()
            .map(|i| (i.name.as_str(), i.pinned, i.windows.len()))
            .collect();
        assert_eq!(
            summary,
            [
                ("File Explorer", true, 1),
                ("Terminal", true, 0),
                ("chrome", false, 2),
                ("spotify", false, 1),
            ]
        );
        assert_eq!(items[0].key, r"pin:c:\windows\explorer.exe");
        assert_eq!(items[0].exe.as_deref(), Some(EXPLORER));
        let chrome = &items[2];
        assert_eq!(
            chrome.windows.iter().map(|w| w.active).collect::<Vec<_>>(),
            [false, true]
        );
        assert!(items[3].windows[0].attention, "spotify flashes");
        assert_eq!(items[3].aumid.as_deref(), Some("Spotify"));

        use crate::config::Running as Show;
        let pinned_only = View {
            running: Show::None,
            ..View::default()
        };
        assert_eq!(dock_items(&pins, &apps, &order, pinned_only).len(), 2);
        let elsewhere = View {
            running: Show::Monitor,
            monitor: 2,
            ..View::default()
        };
        let items = dock_items(&pins, &apps, &order, elsewhere);
        assert_eq!(items.len(), 2, "no running app has a window on monitor 2");
        assert!(items.iter().all(|i| i.windows.is_empty()));
    }

    /// Discord's pin is `Update.exe --processStart Discord.exe`; its windows run from a version
    /// folder, some with the shortcut's AUMID and some without. All of them group under the pin.
    #[test]
    fn squirrel_apps_group_with_their_pinned_shortcut() {
        let root = r"c:\users\me\appdata\local\discord";
        let pin_exe = squirrel_target(&format!(r"{root}\Update.exe"), r#"--processStart "Discord.exe""#);
        assert_eq!(pin_exe.as_deref(), Some(r"c:\users\me\appdata\local\discord\discord.exe"));
        assert_eq!(
            squirrel_target(r"C:\Slack\Update.exe", "--processStart slack.exe --process-start-args x"),
            Some(r"c:\slack\slack.exe".into())
        );
        assert_eq!(squirrel_target(r"C:\Tools\setup.exe", "--processStart a.exe"), None);
        assert_eq!(squirrel_target(r"C:\X\Update.exe", "--check"), None);

        let running_exe = format!(r"{root}\app-1.0.9188\Discord.exe");
        let groups = group(vec![
            window(20, Some("com.squirrel.Discord.Discord"), Some(running_exe.as_str())),
            window(21, None, Some(running_exe.to_lowercase().as_str())),
            window(22, None, Some(CHROME)),
        ]);
        let mut order = Vec::new();
        update_order(&mut order, &groups);
        let apps: Vec<Running> = groups.into_iter().map(|g| running(g, "x")).collect();
        let pins = [Pin {
            path: r"C:\Users\me\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Discord Inc\Discord.lnk".into(),
            name: "Discord".into(),
            exe: pin_exe,
            ..Pin::default()
        }];
        let items = dock_items(&pins, &apps, &order, View::default());
        let summary: Vec<(&str, bool, usize)> =
            items.iter().map(|i| (i.name.as_str(), i.pinned, i.windows.len())).collect();
        assert_eq!(summary, [("Discord", true, 2), ("x", false, 1)]);
    }

    #[test]
    fn chromium_pins_keep_distinct_appids_and_remain_visible_while_running() {
        let groups = group(vec![
            window(30, Some("Helium"), Some(CHROME)),
            window(31, Some("Helium._crx_chat"), Some(CHROME)),
        ]);
        let apps: Vec<_> = groups.into_iter().map(|g| running(g, "browser")).collect();
        let pins = [
            Pin {
                path: "Helium.lnk".into(),
                name: "Helium".into(),
                aumid: Some("Helium".into()),
                exe: Some(CHROME.into()),
                ..Default::default()
            },
            Pin {
                path: "Chat.lnk".into(),
                name: "Chat".into(),
                aumid: Some("Helium._crx_chat".into()),
                exe: Some(CHROME.into()),
                ..Default::default()
            },
        ];
        let items = dock_items(&pins, &apps, &[], View::default());
        assert_eq!(items.len(), 2);
        assert!(
            items
                .iter()
                .all(|item| item.pinned && item.windows.len() == 1)
        );
        assert_eq!(items[0].windows[0].hwnd, 30);
        assert_eq!(items[1].windows[0].hwnd, 31);
        let without_pin = dock_items(&pins[..1], &apps, &[], View::default());
        assert_eq!(
            without_pin.len(),
            2,
            "an unpinned PWA remains a separate running app"
        );
        let stopped = dock_items(&pins, &[], &[], View::default());
        assert_eq!(
            stopped.len(),
            2,
            "pins stay visible after their windows close"
        );
    }

    /// Notification badges key by AUMID: a grouped Squirrel pin carries its windows' one, and
    /// with no window (tray only) the one Squirrel registers from its install folder.
    #[test]
    fn grouped_squirrel_pins_keep_the_identity_their_notifications_use() {
        let exe = r"c:\users\me\appdata\local\discord\app-1.0.9188\Discord.exe";
        let groups = group(vec![window(20, Some("com.squirrel.Discord.Discord"), Some(exe))]);
        let apps: Vec<Running> = groups.into_iter().map(|g| running(g, "Discord")).collect();
        let pins = [Pin {
            path: r"C:\Discord.lnk".into(),
            exe: squirrel_target(r"c:\users\me\appdata\local\discord\Update.exe", "--processStart Discord.exe"),
            ..Pin::default()
        }];
        let items = dock_items(&pins, &apps, &[], View::default());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].aumid.as_deref(), Some("com.squirrel.Discord.Discord"));
        let tray_only = dock_items(&pins, &[], &[], View::default());
        let id = tray_only[0].exe.as_deref().and_then(squirrel_aumid);
        assert_eq!(id.as_deref(), Some("com.squirrel.discord.discord"));
        assert_eq!(
            squirrel_aumid(r"c:\users\me\appdata\local\slack\app-4.41.0\slack.exe").as_deref(),
            Some("com.squirrel.slack.slack")
        );
        assert_eq!(
            squirrel_aumid(r"c:\users\me\appdata\local\microsoft\teams\current\Teams.exe").as_deref(),
            Some("com.squirrel.teams.teams")
        );
    }

    #[test]
    fn file_stems_are_readable_names() {
        assert_eq!(file_stem(r"C:\Program Files\App\Code.exe"), "Code");
        assert_eq!(file_stem(r"C:\Users\me\Desktop\Notes.lnk"), "Notes");
        assert_eq!(file_stem(r"C:\Users\me\Projects"), "Projects");
        assert_eq!(file_stem(".hidden"), ".hidden");
    }
}
