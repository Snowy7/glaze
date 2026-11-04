//! The launcher's app index: every app the Start menu lists, from `shell:AppsFolder` (packaged
//! apps by AUMID, and the Start Menu folders' shortcuts, user and common), with names, icons and
//! search keywords (a shortcut's target exe: "code" finds Visual Studio Code). Built on the `apps`
//! lane at low priority, cached in `%LOCALAPPDATA%\mypaper\apps.json`, rebuilt when the Start Menu
//! folders change. Also what the launcher remembers (`launcher.json`: what was run, how often and
//! when) and the recent files (`%APPDATA%\Microsoft\Windows\Recent`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use windows::Win32::Graphics::Imaging::{CLSID_WICImagingFactory, IWICImagingFactory};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
};
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, FOLDERID_CommonPrograms, FOLDERID_Programs,
    IEnumShellItems, IShellItem, KF_FLAG_DEFAULT, SHGetKnownFolderItem, SHGetKnownFolderPath,
    SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
};
use windows::core::GUID;

/// One app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct App {
    /// Its AppsFolder parsing name: an AUMID, or a known-folder path for a desktop app.
    pub id: String,
    pub name: String,
    /// What launches it: `shell:AppsFolder\<id>`.
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// More words a search matches (a shortcut's target exe).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// A packaged (Store) app.
    #[serde(default)]
    pub packaged: bool,
}

const CACHE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Cache {
    version: u32,
    apps: Vec<App>,
}

/// The cached index, if it's there and of this version.
pub fn parse_cache(json: &str) -> Option<Vec<App>> {
    let cache: Cache = serde_json::from_str(json).ok()?;
    (cache.version == CACHE_VERSION).then_some(cache.apps)
}

pub fn cache_json(apps: &[App]) -> String {
    #[derive(Serialize)]
    struct Out<'a> {
        version: u32,
        apps: &'a [App],
    }
    serde_json::to_string(&Out {
        version: CACHE_VERSION,
        apps,
    })
    .unwrap_or_default()
}

/// Whether an AppsFolder entry is an app the launcher should list: not an uninstaller, a readme
/// or a web link (the Start Menu folders hold those too).
pub fn keep(id: &str, name: &str) -> bool {
    let name = name.to_lowercase();
    let id = id.to_lowercase();
    let docs = [
        ".txt", ".chm", ".pdf", ".htm", ".html", ".rtf", ".md", ".url", ".ini",
    ];
    !name.is_empty()
        && !name.starts_with("uninstall")
        && !name.contains("uninstaller")
        && !id.starts_with("http:")
        && !id.starts_with("https:")
        && !docs.iter().any(|ext| id.ends_with(ext))
}

/// A Start Menu shortcut: its name (the file name) and its target exe's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub name: String,
    pub target: String,
}

/// Builds the index from the AppsFolder entries (`(id, name)`), adding each shortcut's target exe
/// as a keyword to the app of the same name. Sorted by name; one entry per id.
pub fn build(entries: Vec<(String, String)>, links: &[Link]) -> Vec<App> {
    let targets: HashMap<String, &str> = links
        .iter()
        .map(|l| (l.name.to_lowercase(), l.target.as_str()))
        .collect();
    let mut seen = std::collections::HashSet::new();
    let mut apps: Vec<App> = entries
        .into_iter()
        .filter(|(id, name)| keep(id, name) && seen.insert(id.to_lowercase()))
        .map(|(id, name)| {
            let mut keywords = Vec::new();
            if let Some(target) = targets.get(&name.to_lowercase())
                && !target.is_empty()
                && !name.to_lowercase().contains(&target.to_lowercase())
            {
                keywords.push((*target).to_owned());
            }
            App {
                path: format!(r"shell:AppsFolder\{id}"),
                packaged: id.contains('!'),
                keywords,
                icon: None,
                name,
                id,
            }
        })
        .collect();
    apps.sort_by_cached_key(|a| a.name.to_lowercase());
    apps
}

// --- What the launcher remembers ---------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    pub id: String,
    pub path: String,
    pub count: u32,
    /// Milliseconds since the epoch.
    pub last: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    #[serde(default)]
    pub recent: Vec<Recent>,
}

/// How many runs are remembered.
const HISTORY: usize = 60;

impl History {
    /// Remembers a run: counted, moved to the front; the oldest fall off.
    pub fn record(&mut self, id: &str, path: &str, now: u64) {
        let count = match self.recent.iter().position(|r| r.id == id) {
            Some(at) => self.recent.remove(at).count,
            None => 0,
        };
        self.recent.insert(
            0,
            Recent {
                id: id.to_owned(),
                path: path.to_owned(),
                count: count.saturating_add(1),
                last: now,
            },
        );
        self.recent.truncate(HISTORY);
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        let text = serde_json::to_string_pretty(self).unwrap_or_default() + "\n";
        if let Err(e) = mypaper_bars::files::write_atomic(path, text.as_bytes()) {
            log::warn!("launcher: not saving the history: {e:#}");
        }
    }
}

/// The signed-in user's display name ("Ada Lovelace"), else their account name.
pub fn user_name() -> String {
    use windows::Win32::Security::Authentication::Identity::{GetUserNameExW, NameDisplay};
    let mut buffer = [0u16; 256];
    let mut len = buffer.len() as u32;
    // SAFETY: the buffer and its length outlive the call.
    let ok = unsafe {
        GetUserNameExW(
            NameDisplay,
            Some(windows::core::PWSTR(buffer.as_mut_ptr())),
            &mut len,
        )
    };
    let name = ok
        .then(|| String::from_utf16_lossy(&buffer[..len as usize]))
        .filter(|n| !n.trim().is_empty());
    name.or_else(|| std::env::var("USERNAME").ok())
        .unwrap_or_else(|| "User".into())
}

pub fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

// --- Building it (on the apps lane) -------------------------------------------------------------

/// The apps lane's state: COM, WIC for icons, and where icons go.
pub struct Builder {
    wic: Option<IWICImagingFactory>,
    icons: PathBuf,
}

impl Builder {
    /// Runs on the lane: COM (shell items want an STA) at background priority.
    pub fn new(icons: PathBuf) -> Self {
        // SAFETY: COM for this thread, kept for its life; lowering our own thread's priority.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
        }
        // SAFETY: plain COM creation.
        let wic = unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
            .inspect_err(|e| log::warn!("launcher: no icons (WIC): {e}"))
            .ok();
        let _ = std::fs::create_dir_all(&icons);
        Self { wic, icons }
    }

    /// Enumerates the apps, names them and draws their icons (cached on disk by path).
    pub fn build(&self) -> Vec<App> {
        let started = std::time::Instant::now();
        let mut items = HashMap::new();
        let entries: Vec<(String, String)> = apps_folder()
            .into_iter()
            .map(|(id, name, item)| {
                items.insert(id.clone(), item);
                (id, name)
            })
            .collect();
        let mut apps = build(entries, &start_menu_links());
        if let Some(wic) = &self.wic {
            for app in &mut apps {
                if let Some(item) = items.get(&app.id) {
                    app.icon = crate::icons::icon(item, &app.path.to_lowercase(), wic, &self.icons);
                }
            }
        }
        log::info!(
            "launcher: {} apps indexed in {:?}",
            apps.len(),
            started.elapsed()
        );
        apps
    }

    /// The recent files, newest first, with icons.
    pub fn recent_files(&self, limit: usize) -> Vec<RecentFile> {
        let Some(dir) =
            std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join(r"Microsoft\Windows\Recent"))
        else {
            return Vec::new();
        };
        let mut files: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("lnk"))
            })
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        files.sort_by_key(|f| std::cmp::Reverse(f.0));
        files
            .into_iter()
            .take(limit)
            .map(|(modified, path)| {
                let path = path.display().to_string();
                let icon = self.wic.as_ref().and_then(|wic| {
                    let item = crate::icons::shell_item(&path)?;
                    crate::icons::icon(&item, &path.to_lowercase(), wic, &self.icons)
                });
                RecentFile {
                    name: crate::apps::file_stem(&path),
                    at: modified
                        .duration_since(UNIX_EPOCH)
                        .map_or(0, |d| d.as_millis() as u64),
                    icon,
                    path,
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecentFile {
    pub name: String,
    /// The shortcut in Recent, which opens the file.
    pub path: String,
    /// Last used, ms since the epoch.
    pub at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

fn display(item: &IShellItem, how: windows::Win32::UI::Shell::SIGDN) -> Option<String> {
    // SAFETY: COM call; the returned string is ours to free.
    unsafe {
        let name = item.GetDisplayName(how).ok()?;
        let text = name.to_string().ok();
        CoTaskMemFree(Some(name.0.cast()));
        text.filter(|t| !t.is_empty())
    }
}

/// Every entry of `shell:AppsFolder`: its parsing name, display name and item.
fn apps_folder() -> Vec<(String, String, IShellItem)> {
    let mut out = Vec::new();
    // SAFETY: COM calls on objects we own; `next` gets one item at a time.
    unsafe {
        let Ok(folder) =
            SHGetKnownFolderItem::<IShellItem>(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)
        else {
            log::warn!("launcher: no AppsFolder");
            return out;
        };
        let Ok(items) = folder.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) else {
            return out;
        };
        loop {
            let mut next = [None];
            let mut fetched = 0u32;
            if items.Next(&mut next, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = next[0].take() else { break };
            if let (Some(id), Some(name)) = (
                display(&item, SIGDN_PARENTRELATIVEPARSING),
                display(&item, SIGDN_NORMALDISPLAY),
            ) {
                out.push((id, name, item));
            }
        }
    }
    out
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    // SAFETY: the returned string is ours to free.
    unsafe {
        let path = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let text = path.to_string().ok();
        CoTaskMemFree(Some(path.0.cast()));
        text.map(PathBuf::from)
    }
}

/// The Start Menu folders (user and common) the index follows.
pub fn start_menu_dirs() -> Vec<PathBuf> {
    [FOLDERID_Programs, FOLDERID_CommonPrograms]
        .iter()
        .filter_map(known_folder)
        .collect()
}

/// The Start Menu shortcuts and their target exes' names.
fn start_menu_links() -> Vec<Link> {
    let mut links = Vec::new();
    let mut dirs = start_menu_dirs();
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("lnk"))
            {
                let text = path.display().to_string();
                let (target, _) = crate::icons::resolve_link(&text);
                if let Some(target) = target.filter(|t| t.ends_with(".exe")) {
                    links.push(Link {
                        name: crate::apps::file_stem(&text),
                        target: crate::apps::file_stem(&target),
                    });
                }
            }
        }
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, name: &str) -> (String, String) {
        (id.into(), name.into())
    }

    #[test]
    fn the_index_keeps_apps_and_drops_the_rest() {
        let entries = vec![
            entry("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App", "Terminal"),
            entry(
                r"{6D809377-6AF0-444B-8957-A3773F02200E}\Code\Code.exe",
                "Visual Studio Code",
            ),
            entry(
                r"{7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E}\App\unins000.exe",
                "Uninstall App",
            ),
            entry(r"C:\Program Files\App\README.txt", "Read me"),
            entry("https://example.com/", "Example website"),
            entry(
                "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
                "Terminal (again)",
            ),
            entry("Microsoft.Windows.Explorer", "File Explorer"),
        ];
        let links = [
            Link {
                name: "Visual Studio Code".into(),
                target: "Code".into(),
            },
            Link {
                name: "Microsoft Edge".into(),
                target: "msedge".into(),
            },
        ];
        let mut entries = entries;
        entries.push(entry("MSEdge", "Microsoft Edge"));
        let apps = build(entries, &links);
        let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "File Explorer",
                "Microsoft Edge",
                "Terminal",
                "Visual Studio Code"
            ]
        );
        assert_eq!(apps[1].keywords, ["msedge"], "the exe's name is a keyword");
        assert_eq!(
            apps[3].keywords,
            Vec::<String>::new(),
            "\"code\" is in the name already"
        );
        assert!(apps[2].packaged && !apps[0].packaged);
        assert_eq!(
            apps[2].path,
            r"shell:AppsFolder\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"
        );
    }

    #[test]
    fn the_cache_round_trips_and_refuses_other_versions() {
        let apps = build(vec![entry("A!App", "Alpha")], &[]);
        let json = cache_json(&apps);
        assert_eq!(parse_cache(&json), Some(apps));
        assert_eq!(parse_cache(r#"{ "version": 0, "apps": [] }"#), None);
        assert_eq!(parse_cache("not json"), None);
        assert_eq!(
            parse_cache(r#"{ "version": 1, "apps": [ { "id": "x", "name": "X", "path": "p" } ] }"#)
                .map(|a| a.len()),
            Some(1),
            "optional fields may be missing"
        );
    }

    #[test]
    fn the_history_counts_and_keeps_the_newest_first() {
        let mut history = History::default();
        history.record("a", "pa", 1);
        history.record("b", "pb", 2);
        history.record("a", "pa", 3);
        let order: Vec<(&str, u32, u64)> = history
            .recent
            .iter()
            .map(|r| (r.id.as_str(), r.count, r.last))
            .collect();
        assert_eq!(order, [("a", 2, 3), ("b", 1, 2)]);
        for i in 0..100 {
            history.record(&format!("x{i}"), "p", 10 + i);
        }
        assert_eq!(history.recent.len(), HISTORY);
        assert_eq!(history.recent[0].id, "x99");
    }
}
