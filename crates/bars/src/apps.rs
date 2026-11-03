//! The windows a taskbar would show, grouped into apps, and the dock's items built from them and
//! the pins (docs/bars.md, "Dock").
//!
//! The rules (eligibility, grouping, ordering) are pure functions; [`Apps`] reads the real
//! windows when the hooks say something changed.

use std::collections::HashMap;

use serde::Serialize;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
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
        (None, Some(exe)) => format!("exe:{exe}"),
        (None, None) => format!("window:{}", window.hwnd),
    }
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
    fn matches(&self, group: &Group) -> bool {
        let same = |a: &Option<String>, b: &Option<String>| matches!((a, b), (Some(a), Some(b)) if a.eq_ignore_ascii_case(b));
        same(&self.aumid, &group.aumid) || same(&self.exe, &group.exe)
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
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ItemWindow {
    pub hwnd: isize,
    pub title: String,
    pub active: bool,
    pub minimized: bool,
}

/// The key of a pinned item: its path, case-insensitively.
pub fn pin_key(path: &str) -> String {
    format!("pin:{}", path.to_lowercase())
}

/// The dock: pinned items in order (with the windows of every app they match), then, if
/// `running`, the other running apps in `order`. `active` is the window in front.
pub fn dock_items(
    pins: &[Pin],
    running: &[Running],
    order: &[String],
    show_running: bool,
    active: Option<isize>,
) -> Vec<Item> {
    let windows_of = |groups: &[&Group]| -> Vec<ItemWindow> {
        groups
            .iter()
            .flat_map(|g| &g.windows)
            .map(|w| ItemWindow {
                hwnd: w.hwnd,
                title: w.title.clone(),
                active: Some(w.hwnd) == active,
                minimized: w.minimized,
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
            }
        })
        .collect();
    if show_running {
        let mut rest: Vec<&Running> = running
            .iter()
            .zip(&claimed)
            .filter(|(_, claimed)| !**claimed)
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

fn facts(hwnd: HWND, ours: u32) -> (Facts, u32) {
    // SAFETY: read-only queries on a handle from EnumWindows.
    unsafe {
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
    }
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

/// The process image path, lowercase.
fn process_image(pid: u32) -> Option<String> {
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
        let items = dock_items(&pins, &apps, &order, true, Some(13));
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
        let chrome = &items[2];
        assert_eq!(
            chrome.windows.iter().map(|w| w.active).collect::<Vec<_>>(),
            [false, true]
        );

        let pinned_only = dock_items(&pins, &apps, &order, false, None);
        assert_eq!(pinned_only.len(), 2);
    }

    #[test]
    fn file_stems_are_readable_names() {
        assert_eq!(file_stem(r"C:\Program Files\App\Code.exe"), "Code");
        assert_eq!(file_stem(r"C:\Users\me\Desktop\Notes.lnk"), "Notes");
        assert_eq!(file_stem(r"C:\Users\me\Projects"), "Projects");
        assert_eq!(file_stem(".hidden"), ".hidden");
    }
}
