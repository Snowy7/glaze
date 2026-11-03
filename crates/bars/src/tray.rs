//! The tray module, best effort (docs/bars.md, "Modules"). Notification-area icons belong to
//! Explorer; this reads them through UI Automation, a supported accessibility API, from the
//! taskbar (hidden or not), on its own MTA worker thread while a tray module is on screen.
//!
//! What Windows 11 exposes: each promoted icon is a `SystemTray.NormalButton` with AutomationId
//! `NotifyItemIcon` (its Name is the tooltip) that supports `Invoke`; the overflow chevron is the
//! `SystemTrayIcon` button left of them. Hidden icons only exist in the tree while the overflow
//! flyout is open, so they're reached through the chevron, which is always listed last as
//! `overflow`. UIA has no icons: each item gets the `IconSnapshot` PNG the shell keeps under
//! `HKCU\Control Panel\NotifyIconSettings` when its tooltip or exe matches the name.
//!
//! Clicks: left is UIA `Invoke`; right focuses the button and sends the context-menu key
//! (Shift+F10), which Explorer forwards to the app as its context menu. If UI Automation fails,
//! the module shows only `overflow`, which opens the flyout with Win+B, Enter.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use serde::Serialize;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern,
    IUIAutomationStructureChangedEventHandler, IUIAutomationStructureChangedEventHandler_Impl,
    StructureChangeType, TreeScope_Descendants, TreeScope_Subtree, UIA_AutomationIdPropertyId,
    UIA_ClassNamePropertyId, UIA_InvokePatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_B, VK_F10, VK_LWIN, VK_RETURN, VK_SHIFT,
};
use windows::Win32::UI::Shell::{KF_FLAG_DEFAULT, SHGetKnownFolderPath};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
use windows::core::{GUID, HSTRING, PCWSTR, implement, w};

use crate::bridge::Button;
use crate::wake::{Msg, Waker};

/// The id of the overflow chevron (and of the fallback button).
pub const OVERFLOW: &str = "overflow";
/// Coalesces the burst of UIA events one tray change produces.
const SETTLE: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrayItem {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

enum Cmd {
    Refresh,
    Click(String, Button),
    Stop,
}

/// The worker while a tray module is on screen.
pub struct Tray {
    worker: Option<Sender<Cmd>>,
    waker: Waker,
    icons: PathBuf,
}

impl Tray {
    pub fn new(waker: Waker, icons: PathBuf) -> Self {
        Self {
            worker: None,
            waker,
            icons,
        }
    }

    pub fn set_active(&mut self, on: bool) {
        match (on, self.worker.is_some()) {
            (true, false) => {
                let (tx, rx) = mpsc::channel();
                let (waker, icons, again) = (self.waker.clone(), self.icons.clone(), tx.clone());
                let spawned = std::thread::Builder::new()
                    .name("tray".into())
                    .spawn(move || run(&rx, &again, &waker, &icons));
                match spawned {
                    Ok(_) => self.worker = Some(tx),
                    Err(e) => log::warn!("no tray worker: {e}"),
                }
            }
            (false, true) => {
                if let Some(worker) = self.worker.take() {
                    let _ = worker.send(Cmd::Stop);
                }
            }
            _ => {}
        }
    }

    /// Explorer restarted: find the new taskbar.
    pub fn refresh(&self) {
        if let Some(worker) = &self.worker {
            let _ = worker.send(Cmd::Refresh);
        }
    }

    pub fn click(&self, id: String, button: Button) {
        match &self.worker {
            Some(worker) => {
                let _ = worker.send(Cmd::Click(id, button));
            }
            None if id == OVERFLOW => open_overflow_by_keyboard(),
            None => {}
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.set_active(false);
    }
}

#[implement(IUIAutomationStructureChangedEventHandler)]
struct Changed(Sender<Cmd>);

impl IUIAutomationStructureChangedEventHandler_Impl for Changed_Impl {
    fn HandleStructureChangedEvent(
        &self,
        _: windows::core::Ref<IUIAutomationElement>,
        _: StructureChangeType,
        _: *const windows::Win32::System::Com::SAFEARRAY,
    ) -> windows::core::Result<()> {
        let _ = self.0.send(Cmd::Refresh);
        Ok(())
    }
}

/// The taskbar seen through UI Automation.
struct Taskbar {
    uia: IUIAutomation,
    root: IUIAutomationElement,
    handler: IUIAutomationStructureChangedEventHandler,
}

impl Taskbar {
    fn find(uia: &IUIAutomation, again: &Sender<Cmd>) -> Option<Self> {
        // SAFETY: COM calls on objects we own; the handler is removed on drop.
        unsafe {
            let tray = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).ok()?;
            let root = uia.ElementFromHandle(tray).ok()?;
            let handler: IUIAutomationStructureChangedEventHandler = Changed(again.clone()).into();
            if let Err(e) =
                uia.AddStructureChangedEventHandler(&root, TreeScope_Subtree, None, &handler)
            {
                log::info!("tray: no change events ({e}); it updates when Explorer restarts");
            }
            Some(Self {
                uia: uia.clone(),
                root,
                handler,
            })
        }
    }

    /// The promoted icons, then the overflow chevron.
    fn buttons(&self) -> Vec<(String, IUIAutomationElement)> {
        // SAFETY: COM calls on live UIA objects.
        unsafe {
            let all = |property, value: &str| -> Vec<IUIAutomationElement> {
                let Ok(condition) = self
                    .uia
                    .CreatePropertyCondition(property, &VARIANT::from(value))
                else {
                    return Vec::new();
                };
                let Ok(found) = self.root.FindAll(TreeScope_Descendants, &condition) else {
                    return Vec::new();
                };
                let count = found.Length().unwrap_or(0);
                (0..count)
                    .filter_map(|i| found.GetElement(i).ok())
                    .collect()
            };
            let name = |element: &IUIAutomationElement| {
                element
                    .CurrentName()
                    .map(|n| n.to_string().trim().to_owned())
                    .unwrap_or_default()
            };
            let icons = all(UIA_AutomationIdPropertyId, "NotifyItemIcon");
            let mut buttons: Vec<(String, IUIAutomationElement)> =
                icons.into_iter().map(|e| (name(&e), e)).collect();
            // The chevron: the plain system-tray button left of every promoted icon.
            let left = |element: &IUIAutomationElement| {
                element
                    .CurrentBoundingRectangle()
                    .map_or(i32::MAX, |r| r.left)
            };
            let first_icon = buttons
                .iter()
                .map(|(_, e)| left(e))
                .min()
                .unwrap_or(i32::MAX);
            let chevron = all(UIA_ClassNamePropertyId, "SystemTray.NormalButton")
                .into_iter()
                .filter(|e| {
                    e.CurrentAutomationId()
                        .is_ok_and(|id| id == "SystemTrayIcon")
                })
                .filter(|e| left(e) <= first_icon)
                .min_by_key(left);
            if let Some(chevron) = chevron {
                buttons.push((OVERFLOW.to_owned(), chevron));
            }
            buttons
        }
    }
}

impl Drop for Taskbar {
    fn drop(&mut self) {
        // SAFETY: removes the handler we added on this element.
        let _ = unsafe {
            self.uia
                .RemoveStructureChangedEventHandler(&self.root, &self.handler)
        };
    }
}

fn run(rx: &Receiver<Cmd>, again: &Sender<Cmd>, waker: &Waker, icons: &std::path::Path) {
    // SAFETY: COM for this thread; balanced below.
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    // SAFETY: plain COM creation.
    let uia: Option<IUIAutomation> =
        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
            .inspect_err(|e| {
                log::warn!(
                    "tray: UI Automation unavailable ({e}); showing the overflow button only"
                )
            })
            .ok();
    let mut taskbar = uia.as_ref().and_then(|uia| Taskbar::find(uia, again));
    let mut descriptions = HashMap::new();
    let mut sent = None;
    let mut report = |taskbar: &Option<Taskbar>,
                      descriptions: &mut HashMap<String, Option<String>>| {
        let items = match taskbar {
            Some(taskbar) => items(taskbar, descriptions, icons),
            None => vec![fallback()],
        };
        if sent.as_ref() != Some(&items) {
            if items.len() <= 1 && taskbar.is_some() {
                log::info!("tray: no notification icons found through UI Automation");
            }
            waker.send(Msg::Tray(items.clone()));
            sent = Some(items);
        }
    };
    report(&taskbar, &mut descriptions);
    let mut dirty = false;
    loop {
        let command = if dirty {
            match rx.recv_timeout(SETTLE) {
                Ok(command) => command,
                Err(RecvTimeoutError::Timeout) => {
                    dirty = false;
                    report(&taskbar, &mut descriptions);
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(command) => command,
                Err(_) => break,
            }
        };
        match command {
            Cmd::Refresh => {
                if taskbar.is_none()
                    && let Some(uia) = &uia
                {
                    taskbar = Taskbar::find(uia, again);
                }
                dirty = true;
            }
            Cmd::Click(id, button) => {
                let clicked = taskbar.as_ref().is_some_and(|t| click(t, &id, button));
                if !clicked && id == OVERFLOW {
                    open_overflow_by_keyboard();
                } else if !clicked {
                    // Explorer may have restarted under us.
                    taskbar = uia.as_ref().and_then(|uia| Taskbar::find(uia, again));
                    dirty = true;
                }
            }
            Cmd::Stop => break,
        }
    }
    drop(taskbar);
    drop(uia);
    if com {
        // SAFETY: balances the CoInitializeEx above, on the same thread.
        unsafe { CoUninitialize() };
    }
}

fn fallback() -> TrayItem {
    TrayItem {
        id: OVERFLOW.to_owned(),
        name: "Show hidden icons".to_owned(),
        icon: None,
    }
}

/// Unique ids from names: a second icon with the same name gets `#2`.
fn ids(names: &[String]) -> Vec<String> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    names
        .iter()
        .map(|name| {
            let count = seen.entry(name.as_str()).or_default();
            *count += 1;
            if *count == 1 {
                name.clone()
            } else {
                format!("{name}#{count}")
            }
        })
        .collect()
}

fn items(
    taskbar: &Taskbar,
    descriptions: &mut HashMap<String, Option<String>>,
    icons: &std::path::Path,
) -> Vec<TrayItem> {
    let buttons = taskbar.buttons();
    let known = known_icons(descriptions);
    let names: Vec<String> = buttons.iter().map(|(name, _)| name.clone()).collect();
    buttons
        .iter()
        .zip(ids(&names))
        .map(|((name, element), id)| {
            if id == OVERFLOW {
                // SAFETY: a COM call on a live element.
                let label = unsafe { element.CurrentName() }
                    .map(|n| n.to_string())
                    .ok()
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| fallback().name);
                return TrayItem {
                    id,
                    name: label,
                    icon: None,
                };
            }
            let icon = match_icon(name, &known)
                .and_then(|k| k.png.as_deref())
                .and_then(|png| crate::icons::store_png(icons, png));
            TrayItem {
                id,
                name: name.clone(),
                icon,
            }
        })
        .collect()
}

fn click(taskbar: &Taskbar, id: &str, button: Button) -> bool {
    let buttons = taskbar.buttons();
    let names: Vec<String> = buttons.iter().map(|(name, _)| name.clone()).collect();
    let Some((_, element)) = buttons
        .iter()
        .zip(ids(&names))
        .find(|(_, candidate)| candidate == id)
        .map(|(button, _)| button)
    else {
        return false;
    };
    // SAFETY: COM calls on a live element.
    unsafe {
        match button {
            Button::Left => element
                .GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
                .and_then(|invoke| invoke.Invoke())
                .inspect_err(|e| log::info!("tray: invoking {id:?} failed: {e}"))
                .is_ok(),
            Button::Right => {
                if element.SetFocus().is_err() {
                    return false;
                }
                keys(&[VK_SHIFT, VK_F10]);
                true
            }
        }
    }
}

/// Win+B focuses the tray's chevron; Enter opens the overflow flyout.
fn open_overflow_by_keyboard() {
    keys(&[VK_LWIN, VK_B]);
    keys(&[VK_RETURN]);
}

fn keys(chord: &[VIRTUAL_KEY]) {
    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                ..Default::default()
            },
        },
    };
    let inputs: Vec<INPUT> = chord
        .iter()
        .map(|&vk| key(vk, false))
        .chain(chord.iter().rev().map(|&vk| key(vk, true)))
        .collect();
    // SAFETY: a valid input array and its element size.
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
}

// --- Icons from the shell's notification-icon settings ---------------------------------------

/// One `NotifyIconSettings` entry.
#[derive(Debug, Clone, Default, PartialEq)]
struct Known {
    tooltip: String,
    /// The exe's file name without `.exe`.
    stem: String,
    /// The exe's `FileDescription`, when readable.
    description: Option<String>,
    promoted: bool,
    png: Option<Vec<u8>>,
}

/// The entry that best fits a tray button's name: its tooltip starts the name, else the exe's
/// description or file name appears in it. Promoted entries win ties.
fn match_icon<'a>(name: &str, known: &'a [Known]) -> Option<&'a Known> {
    let name = name.trim().to_lowercase();
    let score = |k: &Known| {
        let has = |part: &str| part.len() >= 3 && name.contains(&part.to_lowercase());
        if !k.tooltip.is_empty() && name.starts_with(&k.tooltip.trim().to_lowercase()) {
            3
        } else if k.description.as_deref().is_some_and(has) {
            2
        } else if has(&k.stem) {
            1
        } else {
            0
        }
    };
    known
        .iter()
        .filter(|k| k.png.is_some())
        .map(|k| (score(k), k.promoted, k))
        .filter(|(score, ..)| *score > 0)
        .max_by_key(|(score, promoted, _)| (*score, *promoted))
        .map(|(.., k)| k)
}

fn known_icons(descriptions: &mut HashMap<String, Option<String>>) -> Vec<Known> {
    const KEY: PCWSTR = w!(r"Control Panel\NotifyIconSettings");
    let mut out = Vec::new();
    // SAFETY: registry reads into buffers that outlive each call; the key is closed below.
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, KEY, None, KEY_READ, &mut key).is_err() {
            return out;
        }
        let mut index = 0;
        loop {
            let mut name = [0u16; 64];
            let mut len = name.len() as u32;
            let status = RegEnumKeyExW(
                key,
                index,
                Some(windows::core::PWSTR(name.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            );
            if status.is_err() {
                break;
            }
            index += 1;
            let sub = HSTRING::from(String::from_utf16_lossy(&name[..len as usize]));
            let exe = reg_string(key, &sub, w!("ExecutablePath")).unwrap_or_default();
            let path = resolve_known_folder(&exe);
            let description = descriptions
                .entry(path.clone())
                .or_insert_with(|| crate::icons::version_description(&path))
                .clone();
            out.push(Known {
                tooltip: reg_string(key, &sub, w!("InitialTooltip")).unwrap_or_default(),
                stem: crate::apps::file_stem(&exe),
                description,
                promoted: reg_dword(key, &sub, w!("IsPromoted")) == Some(1),
                png: reg_binary(key, &sub, w!("IconSnapshot")),
            });
        }
        let _ = RegCloseKey(key);
    }
    out
}

/// `{6D809377-…}\App\app.exe` (a known-folder id, as the shell stores it) → a real path.
fn resolve_known_folder(path: &str) -> String {
    let Some((id, rest)) = path.strip_prefix('{').and_then(|p| p.split_once('}')) else {
        return path.to_owned();
    };
    let Ok(guid) = GUID::try_from(id) else {
        return path.to_owned();
    };
    // SAFETY: the returned string is ours to free.
    unsafe {
        match SHGetKnownFolderPath(&guid, KF_FLAG_DEFAULT, None) {
            Ok(folder) => {
                let base = folder.to_string().unwrap_or_default();
                CoTaskMemFree(Some(folder.0.cast()));
                format!("{base}{rest}")
            }
            Err(_) => path.to_owned(),
        }
    }
}

fn reg_string(key: HKEY, sub: &HSTRING, name: PCWSTR) -> Option<String> {
    let mut buffer = [0u16; 1024];
    let mut bytes = size_of_val(&buffer) as u32;
    // SAFETY: the buffer and its size outlive the call.
    unsafe {
        RegGetValueW(
            key,
            sub,
            name,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
    }
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..len]))
}

fn reg_dword(key: HKEY, sub: &HSTRING, name: PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut bytes = size_of::<u32>() as u32;
    // SAFETY: as above.
    unsafe {
        RegGetValueW(
            key,
            sub,
            name,
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
    }
    Some(value)
}

fn reg_binary(key: HKEY, sub: &HSTRING, name: PCWSTR) -> Option<Vec<u8>> {
    // SAFETY: two-call pattern; the buffer is as large as the first call reported.
    unsafe {
        let mut bytes = 0u32;
        RegGetValueW(
            key,
            sub,
            name,
            RRF_RT_REG_BINARY,
            None,
            None,
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
        let mut data = vec![0u8; bytes as usize];
        RegGetValueW(
            key,
            sub,
            name,
            RRF_RT_REG_BINARY,
            None,
            Some(data.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
        data.truncate(bytes as usize);
        (data.starts_with(b"\x89PNG")).then_some(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(tooltip: &str, stem: &str, description: Option<&str>, promoted: bool) -> Known {
        Known {
            tooltip: tooltip.into(),
            stem: stem.into(),
            description: description.map(Into::into),
            promoted,
            png: Some(vec![1]),
        }
    }

    #[test]
    fn icons_match_by_tooltip_then_description_then_exe() {
        let entries = [
            known("", "PhoneExperienceHost", Some("Phone Link"), false),
            known("Spotify", "Spotify", None, false),
            known("", "Raycast", None, false),
            known("", "Raycast", None, true),
            known("NVIDIA Settings", "NVDisplay.Container", None, false),
        ];
        let tooltip = |name| match_icon(name, &entries).map(|k| k.tooltip.clone());
        assert_eq!(tooltip("Spotify Free"), Some("Spotify".into()));
        assert_eq!(
            match_icon("Phone Link - Connected", &entries).map(|k| k.stem.as_str()),
            Some("PhoneExperienceHost")
        );
        assert!(
            match_icon("Raycast", &entries).is_some_and(|k| k.promoted),
            "the promoted entry wins a tie"
        );
        assert_eq!(match_icon("Something else", &entries), None);
        assert_eq!(tooltip("NVIDIA Settings"), Some("NVIDIA Settings".into()));
    }

    #[test]
    fn ids_are_unique_names() {
        let names = ["A".to_owned(), "B".to_owned(), "A".to_owned()];
        assert_eq!(ids(&names), ["A", "B", "A#2"]);
    }

    #[test]
    fn known_folder_paths_resolve_or_pass_through() {
        assert_eq!(resolve_known_folder(r"C:\app.exe"), r"C:\app.exe");
        assert_eq!(
            resolve_known_folder("{not-a-guid}\\x.exe"),
            "{not-a-guid}\\x.exe"
        );
        let windows = resolve_known_folder(r"{F38BF404-1D43-42F2-9305-67DE0B28FC23}\explorer.exe");
        assert!(
            windows.to_lowercase().ends_with(r"\windows\explorer.exe"),
            "{windows}"
        );
    }
}
