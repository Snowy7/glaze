//! Keyboard layouts (docs/bars.md, "Flyouts" → "Language"): the installed ones
//! (`GetKeyboardLayoutList`), named from their locale and the registry's layout names, the one the
//! app in front uses, and switching it (`WM_INPUTLANGCHANGEREQUEST` posted to that window, the
//! documented request; the window's own default handling activates the layout).
//!
//! The `language` module follows the layout without polling: on foreground changes, and on the
//! shell's `HSHELL_LANGUAGE` notification (a shell hook window, `RegisterShellHookWindow`).

use std::cell::RefCell;
use std::collections::HashMap;

use serde::Serialize;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Globalization::{
    GetLocaleInfoEx, LCIDToLocaleName, LOCALE_SISO639LANGNAME2, LOCALE_SISO3166CTRYNAME,
    LOCALE_SLOCALIZEDDISPLAYNAME,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
    RegOpenKeyExW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayout, GetKeyboardLayoutList, HKL};
use windows::Win32::UI::Shell::SHLoadIndirectString;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DeregisterShellHookWindow, DestroyWindow,
    GetWindowThreadProcessId, HSHELL_LANGUAGE, PostMessageW, RegisterClassExW,
    RegisterShellHookWindow, RegisterWindowMessageW, WM_INPUTLANGCHANGEREQUEST, WNDCLASSEXW,
    WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use crate::actions::Topic;
use crate::flyouts::{Done, Watched};
use crate::wake::{Msg, Waker};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Layout {
    /// The `HKL`, as hex: what `setLayout` takes.
    pub id: String,
    /// "English (United States)".
    pub language: String,
    /// "US", "United Kingdom", "Microsoft IME".
    pub layout: String,
    /// "ENG".
    pub short: String,
    /// "US".
    pub region: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Languages {
    /// The id of the layout the app in front uses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    pub layouts: Vec<Layout>,
}

/// A layout's id for the page: the `HKL` in hex.
pub fn id_of(hkl: usize) -> String {
    format!("{:08x}", hkl as u32)
}

/// The registry's keyboard layout id (KLID) for an `HKL`. The high word picks it: none or the
/// language itself (`04090409`: `00000409`), a substitute layout (`08090409`: UK on English,
/// `00000809`), an IME (`E0010411`: the whole value), or `Fxxx`, an index the registry maps
/// through `Layout Id` (`by_layout_id`).
pub fn klid(hkl: u32, by_layout_id: impl Fn(u16) -> Option<String>) -> String {
    let language = hkl & 0xFFFF;
    let device = hkl >> 16;
    match device & 0xF000 {
        0xF000 => {
            by_layout_id((device & 0x0FFF) as u16).unwrap_or_else(|| format!("{language:08X}"))
        }
        0xE000 => format!("{hkl:08X}"),
        _ if device == 0 => format!("{language:08X}"),
        _ => format!("{device:08X}"),
    }
}

/// What the registry says about each layout: KLID → (display name, `Layout Id`).
struct Registry {
    names: HashMap<String, String>,
    ids: HashMap<u16, String>,
}

const LAYOUTS: PCWSTR = w!(r"SYSTEM\CurrentControlSet\Control\Keyboard Layouts");

impl Registry {
    fn read() -> Self {
        let mut names = HashMap::new();
        let mut ids = HashMap::new();
        // SAFETY: registry reads into buffers that outlive each call; the key is closed below.
        unsafe {
            let mut key = HKEY::default();
            if RegOpenKeyExW(HKEY_LOCAL_MACHINE, LAYOUTS, None, KEY_READ, &mut key).is_ok() {
                for index in 0.. {
                    let mut name = [0u16; 16];
                    let mut len = name.len() as u32;
                    if RegEnumKeyExW(
                        key,
                        index,
                        Some(PWSTR(name.as_mut_ptr())),
                        &mut len,
                        None,
                        None,
                        None,
                        None,
                    )
                    .is_err()
                    {
                        break;
                    }
                    let klid = String::from_utf16_lossy(&name[..len as usize]).to_uppercase();
                    let sub = HSTRING::from(klid.as_str());
                    let display = string_value(key, &sub, w!("Layout Display Name"))
                        .and_then(|d| indirect(&d))
                        .or_else(|| string_value(key, &sub, w!("Layout Text")));
                    if let Some(display) = display {
                        names.insert(klid.clone(), display);
                    }
                    if let Some(id) = string_value(key, &sub, w!("Layout Id"))
                        .and_then(|id| u16::from_str_radix(id.trim(), 16).ok())
                    {
                        ids.insert(id, klid);
                    }
                }
                let _ = RegCloseKey(key);
            }
        }
        Self { names, ids }
    }
}

fn string_value(key: HKEY, sub: &HSTRING, name: PCWSTR) -> Option<String> {
    let mut buffer = [0u16; 512];
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

fn indirect(text: &str) -> Option<String> {
    if !text.starts_with('@') {
        return Some(text.to_owned());
    }
    let mut buffer = [0u16; 512];
    // SAFETY: valid strings; the buffer outlives the call.
    unsafe { SHLoadIndirectString(&HSTRING::from(text), &mut buffer, None).ok()? };
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(0);
    (len > 0).then(|| String::from_utf16_lossy(&buffer[..len]))
}

fn locale_info(locale: &str, kind: u32) -> Option<String> {
    let name = HSTRING::from(locale);
    let mut buffer = [0u16; 128];
    // SAFETY: valid strings; the buffer outlives the call.
    let len = unsafe { GetLocaleInfoEx(&name, kind, Some(&mut buffer)) };
    (len > 1).then(|| String::from_utf16_lossy(&buffer[..len as usize - 1]))
}

fn locale_name(language: u32) -> Option<String> {
    let mut buffer = [0u16; 85];
    // SAFETY: the buffer outlives the call.
    let len = unsafe { LCIDToLocaleName(language, Some(&mut buffer), 0) };
    (len > 1).then(|| String::from_utf16_lossy(&buffer[..len as usize - 1]))
}

/// The installed layouts, named.
pub fn layouts() -> Vec<Layout> {
    // SAFETY: two-call pattern; the buffer is as large as the first call reported.
    let hkls: Vec<HKL> = unsafe {
        let count = GetKeyboardLayoutList(None);
        let mut list = vec![HKL::default(); count.max(0) as usize];
        let got = GetKeyboardLayoutList(Some(&mut list));
        list.truncate(got.max(0) as usize);
        list
    };
    let registry = Registry::read();
    hkls.into_iter()
        .map(|hkl| {
            let raw = hkl.0 as usize as u32;
            let locale = locale_name(raw & 0xFFFF).unwrap_or_default();
            let klid = klid(raw, |id| registry.ids.get(&id).cloned());
            Layout {
                id: id_of(hkl.0 as usize),
                language: locale_info(&locale, LOCALE_SLOCALIZEDDISPLAYNAME)
                    .unwrap_or_else(|| locale.clone()),
                layout: registry.names.get(&klid).cloned().unwrap_or_default(),
                short: locale_info(&locale, LOCALE_SISO639LANGNAME2)
                    .unwrap_or_default()
                    .to_uppercase(),
                region: locale_info(&locale, LOCALE_SISO3166CTRYNAME).unwrap_or_default(),
            }
        })
        .collect()
}

/// The layout the window's thread uses.
pub fn current(window: Option<isize>) -> Option<String> {
    let window = HWND(window? as *mut _);
    // SAFETY: read-only queries; a stale window reads as thread 0, the caller's layout.
    let hkl = unsafe { GetKeyboardLayout(GetWindowThreadProcessId(window, None)) };
    (!hkl.is_invalid()).then(|| id_of(hkl.0 as usize))
}

/// Asks `window` to switch to the layout `id`.
pub fn switch(window: isize, id: &str) -> Result<(), String> {
    // HKLs are sign-extended on 64-bit (0xF0020409 is 0xFFFFFFFFF0020409).
    let hkl =
        u32::from_str_radix(id, 16).map_err(|_| format!("not a layout: {id}"))? as i32 as isize;
    if !layouts().iter().any(|l| l.id == id) {
        return Err(format!("{id} isn't installed"));
    }
    // SAFETY: posting to a window; a stale one just fails.
    unsafe {
        PostMessageW(
            Some(HWND(window as *mut _)),
            WM_INPUTLANGCHANGEREQUEST,
            WPARAM(0),
            LPARAM(hkl),
        )
    }
    .map_err(|e| e.to_string())
}

thread_local! {
    static WATCH: RefCell<Option<(Waker, u32)>> = const { RefCell::new(None) };
}

/// The shell hook window that hears layout changes, while a `language` module is on screen.
pub struct LanguageWatch {
    window: HWND,
}

impl LanguageWatch {
    /// Call on the main thread.
    pub fn start(waker: Waker) -> Option<Self> {
        // SAFETY: plain registration and a never-shown window of ours, destroyed on drop.
        unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            let class = w!("mypaper.bars.language");
            let info = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(on_shell),
                hInstance: instance.into(),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassExW(&info); // a second start finds it registered
            let window = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class,
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
            .ok()?;
            let message = RegisterWindowMessageW(w!("SHELLHOOK"));
            WATCH.with(|w| *w.borrow_mut() = Some((waker, message)));
            if !RegisterShellHookWindow(window).as_bool() {
                log::info!("language: no shell hook; following foreground changes only");
            }
            Some(Self { window })
        }
    }
}

impl Drop for LanguageWatch {
    fn drop(&mut self) {
        // SAFETY: our window, on its thread.
        unsafe {
            let _ = DeregisterShellHookWindow(self.window);
            let _ = DestroyWindow(self.window);
        }
        WATCH.with(|w| *w.borrow_mut() = None);
    }
}

unsafe extern "system" fn on_shell(
    window: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    WATCH.with(|w| {
        if let Ok(watch) = w.try_borrow()
            && let Some((waker, message)) = watch.as_ref()
            && msg == *message
            && wparam.0 as u32 == HSHELL_LANGUAGE
        {
            waker.send(Msg::Flyout(Done::Changed(Watched::Core(Topic::Language))));
        }
    });
    // SAFETY: default handling of a message we were called with.
    unsafe { DefWindowProcW(window, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_ids_follow_the_hkl_rules() {
        let none = |_| None;
        assert_eq!(klid(0x0409_0409, none), "00000409", "US English");
        assert_eq!(klid(0x0809_0409, none), "00000809", "UK layout on English");
        assert_eq!(klid(0xE001_0411, none), "E0010411", "Japanese IME");
        assert_eq!(klid(0x0000_0407, none), "00000407");
        let by_id = |id: u16| (id == 0x0002).then(|| "00010409".to_owned());
        assert_eq!(
            klid(0xF002_0409, by_id),
            "00010409",
            "US-Dvorak through Layout Id"
        );
        assert_eq!(
            klid(0xF0FF_0409, none),
            "00000409",
            "unknown index: the language's own"
        );
    }

    #[test]
    fn ids_are_hex_hkls() {
        assert_eq!(id_of(0x0409_0409), "04090409");
        assert_eq!(id_of(0xFFFF_FFFF_E001_0411), "e0010411");
        assert_eq!(
            usize::from_str_radix(&id_of(0x0809_0409), 16),
            Ok(0x0809_0409)
        );
    }

    #[test]
    fn the_installed_layouts_have_names() {
        // Every Windows install has at least one layout.
        let layouts = layouts();
        assert!(!layouts.is_empty());
        assert!(
            layouts
                .iter()
                .all(|l| !l.language.is_empty() && !l.short.is_empty()),
            "{layouts:?}"
        );
    }
}
