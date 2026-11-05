//! What the shell feeds component plugins besides faces (docs/plugins.md, "System events",
//! "Desktop menu and shortcuts"):
//!
//! - **System events.** The shell's existing sources (the shared WinEvent hook for window open,
//!   close, focus and move; the display, power and settings broadcasts the bars already watch;
//!   the media provider's lease) are forwarded only while the helper reports a visible
//!   subscriber for the topic ([`Plugins::wants`]), so with no subscriber nothing is hooked or
//!   leased and idle costs nothing.
//! - **Commands.** Desktop-menu items and global shortcuts queue `(plugin, command)` here
//!   ([`invoke`]); the widgets host sends them on its next pass ([`Plugins::command`]).
//! - **Shortcuts.** `commands[].shortcut` of plugins holding `shortcuts`, registered with
//!   `RegisterHotKey` on the shell thread. A combination another program (or mypaper) already
//!   holds is refused with a log line.
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mypaper_desktop::win_events::{Event, Subscription};
use mypaper_integrations::{Data, Lease, Provider};
use mypaper_plugin_api::wire::{SystemEvent, WindowChange};
use mypaper_widgets::manifest::PluginManifest;
use mypaper_widgets::plugins::Plugins;
use serde_json::{Value, json};
use windows::Win32::UI::Input::KeyboardAndMouse::{HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey, UnregisterHotKey};
use windows::Win32::UI::WindowsAndMessaging::{MSG, WM_HOTKEY};

static COMMANDS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Runs `plugin`'s command or menu item `id` on the widgets host's next pass.
pub fn invoke(plugin: &str, id: &str) {
    COMMANDS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((plugin.into(), id.into()));
}

/// A `menu` item or a `commands` entry of an installed component plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct Contribution {
    pub plugin: String,
    /// The plugin's name and icon (its first face's), for the menu's submenu.
    pub name: String,
    pub icon: Option<String>,
    pub id: String,
    pub label: String,
    pub shortcut: Option<String>,
}

/// `manifest`'s `menu` items (with `desktop-menu`) or `commands` (with `shortcuts`); none
/// without the permission.
pub fn contributions_of(manifest: &PluginManifest, menu: bool) -> Vec<Contribution> {
    let permission = if menu { "desktop-menu" } else { "shortcuts" };
    if manifest.component.is_none() || !manifest.permissions.iter().any(|p| p == permission) {
        return Vec::new();
    }
    let icon = manifest.faces.iter().chain(&manifest.bars).chain(&manifest.panels).find_map(|f| f.icon.clone());
    let entries = if menu { &manifest.menu } else { &manifest.commands };
    entries
        .iter()
        .filter_map(|entry| {
            let text = |key: &str| entry.get(key).and_then(Value::as_str).map(str::to_owned);
            Some(Contribution { plugin: manifest.id.clone(), name: manifest.name.clone(), icon: icon.clone(), id: text("id")?, label: text("label")?, shortcut: text("shortcut") })
        })
        .collect()
}

/// Every installed component plugin's (user folder first, then bundled; one per id).
fn installed() -> Vec<(PluginManifest, PathBuf)> {
    let mut seen = std::collections::BTreeSet::new();
    mypaper_widgets::registry::plugin_roots()
        .into_iter()
        .flat_map(|root| std::fs::read_dir(root).into_iter().flatten().flatten())
        .filter_map(|entry| PluginManifest::load(&entry.path()).ok().map(|m| (m, entry.path())))
        .filter(|(m, _)| m.component.is_some() && seen.insert(m.id.clone()))
        .collect()
}

/// Installed plugins' desktop-menu items, grouped by plugin in install order.
pub fn menu_items() -> Vec<Contribution> {
    installed().iter().flat_map(|(m, _)| contributions_of(m, true)).collect()
}

/// `Ctrl+Alt+Shift+Win+<key>` → modifiers and virtual key (letters, digits, F1–F24).
pub fn parse_shortcut(text: &str) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    let mut modifiers = HOT_KEY_MODIFIERS(0);
    let mut key = None;
    for part in text.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= MOD_CONTROL,
            "alt" => modifiers |= MOD_ALT,
            "shift" => modifiers |= MOD_SHIFT,
            "win" | "super" => modifiers |= MOD_WIN,
            k if k.len() == 1 && k.chars().all(|c| c.is_ascii_alphanumeric()) => key = Some(k.to_ascii_uppercase().as_bytes()[0] as u32),
            k => key = Some(k.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()).filter(|n| (1..=24).contains(n)).map(|n| 0x6F + n)?),
        }
    }
    // A bare key would steal typing; a shortcut needs Ctrl, Alt or Win.
    (modifiers.0 & (MOD_CONTROL | MOD_ALT | MOD_WIN).0 != 0).then_some((modifiers | MOD_NOREPEAT, key?))
}

/// Hotkey ids for plugin shortcuts (the shell's own use 1 and 2).
const FIRST_HOTKEY: i32 = 0x5100;

#[derive(Default)]
pub(super) struct Events {
    windows: Option<Subscription>,
    queue: Arc<Mutex<Vec<SystemEvent>>>,
    media: Option<Lease>,
    media_revision: Option<u64>,
    /// Registered shortcuts: hotkey id → `(plugin, command)`.
    shortcuts: Vec<(i32, String, String)>,
    registered: bool,
}

/// Top-level windows already reported (id → title and process), so a hide or destroy is reported
/// as `closed` only for one of them, with the title and process it had.
type Known = std::collections::HashMap<u64, (String, String)>;

/// A WinEvent as a plugin window event: top-level, visible, titled windows only.
fn window_event(event: Event, known: &mut Known) -> Option<SystemEvent> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::*;
    let change = match event.event {
        EVENT_SYSTEM_FOREGROUND => WindowChange::Focused,
        EVENT_OBJECT_SHOW => WindowChange::Opened,
        EVENT_OBJECT_DESTROY | EVENT_OBJECT_HIDE => WindowChange::Closed,
        EVENT_OBJECT_LOCATIONCHANGE => WindowChange::Moved,
        _ => return None,
    };
    let id = event.window as u64;
    if change == WindowChange::Closed {
        // The window may be gone already: it was reported, or it isn't one of ours to report.
        let (title, process) = known.remove(&id)?;
        return Some(SystemEvent::Window { change, id, title, process });
    }
    let hwnd = HWND(event.window as *mut _);
    // SAFETY: plain queries of a window handle; a stale one just fails. Another process's
    // caption is read without sending it a message, so a hung app can't block the shell.
    let title = unsafe {
        if GetAncestor(hwnd, GA_ROOT) != hwnd || !IsWindowVisible(hwnd).as_bool() {
            return None;
        }
        let mut title = [0u16; 256];
        let len = GetWindowTextW(hwnd, &mut title).max(0) as usize;
        String::from_utf16_lossy(&title[..len])
    };
    if title.is_empty() {
        return None;
    }
    let process = match known.get(&id) {
        Some((_, process)) => process.clone(),
        None => process_name(hwnd),
    };
    if known.len() >= 4096 {
        known.clear(); // windows that vanished without a hide; forget them rather than grow
    }
    known.insert(id, (title.clone(), process.clone()));
    Some(SystemEvent::Window { change, id, title, process })
}

/// The image name (`code.exe`) of the process that owns `hwnd`.
fn process_name(hwnd: windows::Win32::Foundation::HWND) -> String {
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let mut pid = 0;
    // SAFETY: the out pointer outlives the call; the process handle is closed below.
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut path = [0u16; 520];
        let mut len = path.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(path.as_mut_ptr()), &mut len).is_ok();
        let _ = windows::Win32::Foundation::CloseHandle(process);
        if !ok {
            return String::new();
        }
        let path = String::from_utf16_lossy(&path[..len as usize]);
        path.rsplit('\\').next().unwrap_or_default().to_ascii_lowercase()
    }
}

impl Events {
    /// Registers every installed plugin's shortcuts (once, on the first pass).
    fn register_shortcuts(&mut self) {
        let commands = installed().iter().flat_map(|(m, _)| contributions_of(m, false)).collect::<Vec<_>>();
        for command in commands {
            let Some(text) = command.shortcut.as_deref() else { continue };
            let Some((modifiers, key)) = parse_shortcut(text) else {
                log::warn!("plugin {}: shortcut {text:?} isn't a key with Ctrl, Alt or Win; ignored", command.plugin);
                continue;
            };
            let id = FIRST_HOTKEY + self.shortcuts.len() as i32;
            // SAFETY: a thread hotkey; WM_HOTKEY arrives in this thread's queue ([`Events::message`]).
            match unsafe { RegisterHotKey(None, id, modifiers, key) } {
                Ok(()) => {
                    log::info!("plugin {}: shortcut {text} runs {:?}", command.plugin, command.id);
                    self.shortcuts.push((id, command.plugin, command.id));
                }
                Err(e) => log::warn!("plugin {}: shortcut {text} refused: another hotkey already uses it ({e})", command.plugin),
            }
        }
    }

    /// A posted message on the shell thread: a plugin shortcut, or a display, power or settings
    /// change the bars already watch.
    pub fn message(&mut self, msg: &MSG) {
        if msg.message == WM_HOTKEY && msg.hwnd.is_invalid() {
            if let Some((_, plugin, id)) = self.shortcuts.iter().find(|(id, ..)| *id as usize == msg.wParam.0) {
                invoke(plugin, id);
            }
            return;
        }
        let event = match msg.message {
            mypaper_bars::events::DISPLAY => SystemEvent::Monitors,
            mypaper_bars::events::POWER => SystemEvent::Power,
            mypaper_bars::events::SETTINGS => SystemEvent::Theme,
            _ => return,
        };
        self.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(event);
    }

    /// Follows the helper's demand: hooks windows and leases media only while wanted, and
    /// forwards what arrived since the last pass.
    pub fn sync(&mut self, plugins: &mut Plugins, now: std::time::Instant) {
        if !std::mem::replace(&mut self.registered, true) {
            self.register_shortcuts();
        }
        for (plugin, id) in std::mem::take(&mut *COMMANDS.lock().unwrap_or_else(std::sync::PoisonError::into_inner)) {
            if let Err(e) = plugins.command(&plugin, &id, now) {
                log::warn!("plugin {plugin}: command {id:?}: {e:?}");
            }
        }
        match (plugins.wants("windows"), self.windows.is_some()) {
            (true, false) => {
                let queue = Arc::clone(&self.queue);
                let known = Mutex::new(Known::new());
                self.windows = Some(Subscription::new(move |event| {
                    let event = window_event(event, &mut known.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
                    if let Some(event) = event {
                        queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(event);
                    }
                }));
            }
            (false, true) => self.windows = None,
            _ => {}
        }
        if plugins.wants("media") {
            if self.media.is_none() {
                self.media = crate::integrations::acquire(Provider::Media).ok();
                self.media_revision = None;
            }
            if let Some(snapshot) = self.media.as_ref().map(Lease::snapshot).filter(|s| self.media_revision != Some(s.revision)) {
                self.media_revision = Some(snapshot.revision);
                if let Some(Data::Media(media)) = snapshot.data {
                    let json = json!({"title": media.title, "artist": media.artist, "album": media.album, "app": media.app_id, "playing": media.playing});
                    plugins.system_event(SystemEvent::Media(json.to_string()));
                }
            }
        } else {
            self.media = None;
        }
        let events = std::mem::take(&mut *self.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        for event in events {
            let topic = match &event {
                SystemEvent::Window { .. } => "windows",
                SystemEvent::Monitors => "monitors",
                SystemEvent::Power => "power",
                SystemEvent::Theme => "theme",
                SystemEvent::Media(_) => "media",
                SystemEvent::Notification(_) => continue,
            };
            if plugins.wants(topic) {
                plugins.system_event(event);
            }
        }
    }
}

impl Drop for Events {
    fn drop(&mut self) {
        for (id, ..) in &self.shortcuts {
            // SAFETY: registered by this thread in `register_shortcuts`.
            let _ = unsafe { UnregisterHotKey(None, *id) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_need_a_modifier_and_a_key() {
        assert_eq!(parse_shortcut("Ctrl+Alt+H"), Some((MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, b'H' as u32)));
        assert_eq!(parse_shortcut("Win+Shift+F5"), Some((MOD_WIN | MOD_SHIFT | MOD_NOREPEAT, 0x74)));
        assert_eq!(parse_shortcut("H"), None);
        assert_eq!(parse_shortcut("Shift+H"), None);
        assert_eq!(parse_shortcut("Ctrl+Alt+Nope"), None);
        assert_eq!(parse_shortcut("Ctrl+Alt"), None);
    }

    #[test]
    fn menu_items_and_shortcuts_are_permission_gated() {
        let manifest = |permissions: &str| {
            let json = format!(r#"{{"id":"hello","name":"Hello","component":"hello.wasm","api":"0.1",
                "faces":[{{"id":"main","name":"Demo","icon":"sparkles","size":{{"default":[2,2]}}}}],
                "menu":[{{"id":"say-hi","label":"Say hi"}}],
                "commands":[{{"id":"say-hi","label":"Say hi","shortcut":"Ctrl+Alt+H"}}],
                "permissions":[{permissions}]}}"#);
            PluginManifest::parse(&json, "hello").unwrap()
        };
        let none = manifest("");
        assert!(contributions_of(&none, true).is_empty() && contributions_of(&none, false).is_empty());
        let both = manifest(r#""desktop-menu","shortcuts""#);
        let item = &contributions_of(&both, true)[0];
        assert_eq!((item.plugin.as_str(), item.name.as_str(), item.icon.as_deref(), item.id.as_str(), item.label.as_str()), ("hello", "Hello", Some("sparkles"), "say-hi", "Say hi"));
        assert_eq!(contributions_of(&both, false)[0].shortcut.as_deref(), Some("Ctrl+Alt+H"));
    }

    #[test]
    fn only_reported_windows_close_with_what_they_were() {
        use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DESTROY;
        let mut known = Known::new();
        let closed = Event { event: EVENT_OBJECT_DESTROY, window: 42 };
        assert_eq!(window_event(closed, &mut known), None, "a child control or an unseen window");
        known.insert(42, ("Notes".into(), "notepad.exe".into()));
        let event = SystemEvent::Window { change: WindowChange::Closed, id: 42, title: "Notes".into(), process: "notepad.exe".into() };
        assert_eq!(window_event(closed, &mut known), Some(event));
        assert!(known.is_empty());
    }

    #[test]
    fn commands_queue_until_the_host_sends_them() {
        invoke("hello", "say-hi");
        assert!(COMMANDS.lock().unwrap().contains(&("hello".into(), "say-hi".into())));
    }
}
