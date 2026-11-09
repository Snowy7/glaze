//! The rest of Windows' own desktop menu: "New" with every registered type, "Paste shortcut",
//! "Undo", and third-party shell extensions (Open in Terminal, the NVIDIA Control Panel, Git
//! Bash...), with their own icons.
//!
//! They come from the desktop folder's background `IContextMenu`, which loads each extension's
//! DLL into this process, so it lives on a **worker thread** of its own (an STA): it builds the
//! menu ahead of time and after every use, flattens it into a [`Snapshot`] the UI thread shows at
//! once, and runs the chosen verb (`InvokeCommand`) on the same `IContextMenu` that made it. The
//! UI thread never waits for it: an extension that hangs leaves the menu without extensions, and
//! a worker stuck longer than [`HANG`] is abandoned for a fresh one.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use glaze_ui::Image;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetDC, GetDIBits, GetObjectW, HBITMAP,
    HGDIOBJ, ReleaseDC, SelectObject,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::Registry::{HKEY_CLASSES_ROOT, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::UI::Shell::{
    ASSOCF_NONE, ASSOCSTR, ASSOCSTR_DEFAULTICON, ASSOCSTR_EXECUTABLE, AssocQueryStringW,
    CMF_NORMAL, CMIC_MASK_PTINVOKE, CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX, GCS_VERBW,
    IContextMenu, IContextMenu2, IContextMenu3, IShellFolder, IShellItemImageFactory,
    SHCreateItemFromParsingName, SHDefExtractIconW, SHGetDesktopFolder, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyIcon, DestroyMenu, GetDesktopWindow, GetIconInfo, GetMenuItemCount,
    GetMenuItemInfoW, HICON, HMENU, ICONINFO, MENUITEMINFOW, MFS_DISABLED, MFT_OWNERDRAW,
    MFT_SEPARATOR, MIIM_BITMAP, MIIM_CHECKMARKS, MIIM_DATA, MIIM_FTYPE, MIIM_ID, MIIM_STATE,
    MIIM_STRING, MIIM_SUBMENU, SW_SHOWNORMAL, WM_DRAWITEM, WM_INITMENUPOPUP, WM_MEASUREITEM,
};
use windows::Win32::Graphics::Gdi::HDC;

// The owner-draw structs of winuser.h (`Win32_UI_Controls` in the windows crate, which this
// crate doesn't otherwise need).
const ODT_MENU: u32 = 1;
const ODA_DRAWENTIRE: u32 = 1;

#[repr(C)]
#[derive(Default)]
#[allow(non_snake_case)]
struct MeasureItemStruct {
    CtlType: u32,
    CtlID: u32,
    itemID: u32,
    itemWidth: u32,
    itemHeight: u32,
    itemData: usize,
}

#[repr(C)]
#[derive(Default)]
#[allow(non_snake_case)]
struct DrawItemStruct {
    CtlType: u32,
    CtlID: u32,
    itemID: u32,
    itemAction: u32,
    itemState: u32,
    hwndItem: HWND,
    hDC: HDC,
    rcItem: RECT,
    itemData: usize,
}
use windows::core::{HSTRING, Interface, PCSTR, PCWSTR, PWSTR};

/// The command ids handed to `QueryContextMenu`: `FIRST..=LAST`.
const FIRST: u32 = 1;
const LAST: u32 = 0x7FFF;
/// A worker busy for longer than this (an extension hung) is given up on.
const HANG: Duration = Duration::from_millis(1500);
/// `CMIC_MASK_UNICODE` (`SEE_MASK_UNICODE`): `lpVerbW` and `lpDirectoryW` are read.
const CMIC_MASK_UNICODE: u32 = 0x4000;

/// Where an item's icon came from (the probe reports it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconFrom {
    /// The verb's `Icon` registry value, extracted at the menu's size.
    Registry,
    /// The item's own bitmap (`hbmpItem`).
    Bitmap,
    /// Drawn by the extension (`HBMMENU_CALLBACK` → `WM_DRAWITEM`).
    Callback,
    /// The item's check-mark bitmaps.
    Checkmark,
    /// The registered type's own icon ("New" items).
    Type,
    /// The program that runs the verb or opens the type.
    Program,
}

/// An item's icon: premultiplied BGRA, the menu's icon size where its source allows.
#[derive(Debug, Clone)]
pub struct ShellIcon {
    pub image: Arc<Image>,
    pub from: IconFrom,
}

/// One item of the shell's menu, flattened.
#[derive(Debug, Clone)]
pub enum ShellItem {
    Separator,
    Action {
        label: String,
        /// The canonical verb, when the extension names one ("paste", "undo", "git_shell").
        verb: String,
        id: u32,
        icon: Option<ShellIcon>,
        enabled: bool,
    },
    Submenu {
        label: String,
        icon: Option<ShellIcon>,
        items: Vec<ShellItem>,
    },
}

/// The shell's menu as it was last built; `generation` names the `IContextMenu` its ids belong to.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub generation: u64,
    pub items: Vec<ShellItem>,
    /// How long building it took.
    pub took: Duration,
}

enum Job {
    /// Build a fresh menu with icons `icon` px square.
    Refresh { icon: i32 },
    Invoke {
        generation: u64,
        id: u32,
        at: (i32, i32),
    },
}

struct Worker {
    jobs: Sender<Job>,
    /// When the job it is running started (`None`: idle).
    busy: Arc<Mutex<Option<Instant>>>,
}

/// The UI thread's handle on the worker.
pub struct ShellMenu {
    worker: Worker,
    latest: Arc<Mutex<Option<Snapshot>>>,
    icon: i32,
}

impl ShellMenu {
    /// Starts the worker and builds the first snapshot with `icon` px icons.
    pub fn start(icon: i32) -> Result<Self> {
        let latest = Arc::new(Mutex::new(None));
        let mut menu = Self {
            worker: spawn(latest.clone())?,
            latest,
            icon,
        };
        menu.refresh();
        Ok(menu)
    }

    /// The last finished snapshot, if any (never waits).
    pub fn snapshot(&self) -> Option<Snapshot> {
        self.latest.lock().ok().and_then(|s| s.clone())
    }

    /// Rebuilds the snapshot in the background (the clipboard, the undo stack or the installed
    /// extensions may have changed). A hung worker is replaced.
    pub fn refresh(&mut self) {
        self.replace_if_hung();
        let _ = self.worker.jobs.send(Job::Refresh { icon: self.icon });
    }

    /// Icons `icon` px square from the next refresh on.
    pub fn set_icon_size(&mut self, icon: i32) {
        if icon != self.icon {
            self.icon = icon;
            self.refresh();
        }
    }

    /// Runs command `id` of snapshot `generation`, as if clicked at `at`, then refreshes.
    pub fn invoke(&mut self, generation: u64, id: u32, at: (i32, i32)) {
        self.replace_if_hung();
        let _ = self.worker.jobs.send(Job::Invoke { generation, id, at });
        let _ = self.worker.jobs.send(Job::Refresh { icon: self.icon });
    }

    fn replace_if_hung(&mut self) {
        let stuck = self
            .worker
            .busy
            .lock()
            .ok()
            .and_then(|b| *b)
            .is_some_and(|since| since.elapsed() > HANG);
        if stuck {
            log::warn!("desktop menu: a shell extension hung; starting a fresh worker without it");
            if let Ok(mut latest) = self.latest.lock() {
                *latest = None;
            }
            // The stuck thread is left to finish (or not) on its own; nothing waits for it.
            match spawn(self.latest.clone()) {
                Ok(worker) => self.worker = worker,
                Err(e) => log::warn!("desktop menu: {e:#}"),
            }
        }
    }
}

fn spawn(latest: Arc<Mutex<Option<Snapshot>>>) -> Result<Worker> {
    let (jobs, rx) = mpsc::channel();
    let busy = Arc::new(Mutex::new(None));
    let flag = busy.clone();
    std::thread::Builder::new()
        .name("desktop-menu-shell".into())
        .spawn(move || work(rx, latest, flag))
        .context("spawning the shell menu worker")?;
    Ok(Worker { jobs, busy })
}

/// A built `IContextMenu` and the `HMENU` it filled.
struct Built {
    generation: u64,
    menu: IContextMenu,
    hmenu: HMENU,
}

impl Drop for Built {
    fn drop(&mut self) {
        // SAFETY: our own popup menu.
        let _ = unsafe { DestroyMenu(self.hmenu) };
    }
}

fn work(
    jobs: Receiver<Job>,
    latest: Arc<Mutex<Option<Snapshot>>>,
    busy: Arc<Mutex<Option<Instant>>>,
) {
    // SAFETY: this thread's apartment, for the extensions' sake (they expect an STA).
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    // The last two menus: a refresh may land between showing a snapshot and choosing from it.
    let mut built: Vec<Built> = Vec::new();
    let mut generation = 0;
    while let Ok(job) = jobs.recv() {
        if let Ok(mut b) = busy.lock() {
            *b = Some(Instant::now());
        }
        match job {
            Job::Refresh { icon } => {
                // Coalesce a burst of refreshes into one.
                generation += 1;
                let started = Instant::now();
                match build(generation, icon) {
                    Ok((menu, items)) => {
                        let snapshot = Snapshot {
                            generation,
                            items,
                            took: started.elapsed(),
                        };
                        log::debug!(
                            "desktop menu: shell items built in {:?} ({} top-level)",
                            snapshot.took,
                            snapshot.items.len()
                        );
                        if let Ok(mut l) = latest.lock() {
                            *l = Some(snapshot);
                        }
                        built.push(menu);
                        if built.len() > 2 {
                            built.remove(0);
                        }
                    }
                    Err(e) => log::warn!("desktop menu: shell items unavailable: {e:#}"),
                }
            }
            Job::Invoke { generation, id, at } => {
                match built.iter().find(|b| b.generation == generation) {
                    Some(b) => {
                        if let Err(e) = invoke(&b.menu, id, at) {
                            log::warn!("desktop menu: shell command {id}: {e:#}");
                        }
                    }
                    None => log::warn!("desktop menu: shell menu {generation} is gone"),
                }
            }
        }
        if let Ok(mut b) = busy.lock() {
            *b = None;
        }
    }
}

/// The desktop folder's background menu, queried and flattened.
fn build(generation: u64, icon: i32) -> Result<(Built, Vec<ShellItem>)> {
    // SAFETY: documented shell calls on this STA; the menu outlives the walk.
    unsafe {
        let desktop: IShellFolder = SHGetDesktopFolder()?;
        let menu: IContextMenu = desktop
            .CreateViewObject(GetDesktopWindow())
            .context("the desktop's background menu")?;
        let hmenu = CreatePopupMenu()?;
        let built = Built {
            generation,
            menu: menu.clone(),
            hmenu,
        };
        menu.QueryContextMenu(hmenu, 0, FIRST, LAST, CMF_NORMAL)
            .ok()?;
        let walker = Walker {
            menu: &menu,
            menu3: menu.cast().ok(),
            menu2: menu.cast().ok(),
            icon,
        };
        let items = walker.walk(hmenu, 0);
        Ok((built, items))
    }
}

struct Walker<'a> {
    menu: &'a IContextMenu,
    menu3: Option<IContextMenu3>,
    menu2: Option<IContextMenu2>,
    icon: i32,
}

impl Walker<'_> {
    /// The items of `hmenu` (a submenu is populated first, as Windows does when it opens).
    unsafe fn walk(&self, hmenu: HMENU, depth: u32) -> Vec<ShellItem> {
        let mut out = Vec::new();
        // SAFETY: (all below) queries on a live menu with valid buffers.
        let count = unsafe { GetMenuItemCount(Some(hmenu)) };
        for index in 0..count.max(0) as u32 {
            let mut text = [0u16; 260];
            let mut info = MENUITEMINFOW {
                cbSize: size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_FTYPE
                    | MIIM_ID
                    | MIIM_STRING
                    | MIIM_SUBMENU
                    | MIIM_STATE
                    | MIIM_BITMAP
                    | MIIM_CHECKMARKS
                    | MIIM_DATA,
                dwTypeData: PWSTR(text.as_mut_ptr()),
                cch: text.len() as u32 - 1,
                ..Default::default()
            };
            // SAFETY: the live menu and writable text buffer belong to this walk.
            if unsafe { GetMenuItemInfoW(hmenu, index, true, &mut info) }.is_err() {
                continue;
            }
            if info.fType.0 & MFT_SEPARATOR.0 != 0 {
                out.push(ShellItem::Separator);
                continue;
            }
            let label = String::from_utf16_lossy(&text[..info.cch as usize]);
            let label = clean(&label);
            if !info.hSubMenu.is_invalid() && depth < 3 {
                // SAFETY: `info` was read from this live menu; its submenu remains owned by it.
                let icon = unsafe { self.icon(hmenu, &info, "") };
                self.init_popup(info.hSubMenu, index);
                // SAFETY: the initialized submenu remains live throughout this recursive walk.
                let items = unsafe { self.walk(info.hSubMenu, depth + 1) };
                if !label.is_empty() && !items.is_empty() {
                    out.push(ShellItem::Submenu { label, icon, items });
                }
                continue;
            }
            // Owner-drawn items with no text (and ids outside ours) are left out.
            if label.is_empty() || info.wID < FIRST || info.wID > LAST {
                if info.fType.0 & MFT_OWNERDRAW.0 != 0 {
                    log::debug!("desktop menu: skipping an owner-drawn item {}", info.wID);
                }
                continue;
            }
            let verb = self.verb(info.wID - FIRST);
            // SAFETY: `info` is this live menu item's metadata and the verb is an owned string.
            let icon = unsafe { self.icon(hmenu, &info, &verb) };
            out.push(ShellItem::Action {
                verb,
                label,
                id: info.wID,
                icon,
                enabled: info.fState.0 & MFS_DISABLED.0 == 0,
            });
        }
        tidy(out)
    }

    /// Lets the extension fill a submenu (the "New" menu fills itself this way).
    fn init_popup(&self, sub: HMENU, index: u32) {
        let (wparam, lparam) = (WPARAM(sub.0 as usize), LPARAM(index as isize));
        // SAFETY: the documented message forwarding of IContextMenu2/3.
        unsafe {
            if let Some(m3) = &self.menu3 {
                let _ = m3.HandleMenuMsg2(WM_INITMENUPOPUP, wparam, lparam, None);
            } else if let Some(m2) = &self.menu2 {
                let _ = m2.HandleMenuMsg(WM_INITMENUPOPUP, wparam, lparam);
            }
        }
    }

    fn verb(&self, offset: u32) -> String {
        let mut buffer = [0u16; 128];
        // SAFETY: a buffer of the length we pass, as the documented GCS_VERBW contract asks.
        let ok = unsafe {
            self.menu.GetCommandString(
                offset as usize,
                GCS_VERBW,
                None,
                windows::core::PSTR(buffer.as_mut_ptr().cast()),
                buffer.len() as u32,
            )
        };
        if ok.is_err() {
            return String::new();
        }
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end]).to_ascii_lowercase()
    }

    /// The item's icon, from the crispest source it has: a "New" type's own icon, a verb's
    /// registered `Icon` (at the menu's size, where the item's bitmap is 16 px), the item's
    /// bitmap, one the extension draws, its check-mark bitmap, else the verb's program.
    unsafe fn icon(&self, hmenu: HMENU, info: &MENUITEMINFOW, verb: &str) -> Option<ShellIcon> {
        let size = self.icon;
        let found = |image: Option<Image>, from| {
            image.map(|image| ShellIcon {
                image: Arc::new(image),
                from,
            })
        };
        // "New" types: the verb is the extension ("newlink" makes a shortcut).
        let extension = match verb {
            "newlink" => Some(".lnk"),
            v if v.starts_with('.') => Some(v),
            _ => None,
        };
        if let Some(extension) = extension
            && let Some(icon) = type_icon(extension, size)
        {
            return Some(icon);
        }
        let key = verb_key(verb);
        if let Some(icon) = key.as_deref().and_then(|k| reg_string(k, "Icon"))
            && let Some(icon) = found(extract(&icon, size), IconFrom::Registry)
        {
            return Some(icon);
        }
        if let Some(icon) = found(bitmap(info.hbmpItem), IconFrom::Bitmap) {
            return Some(icon);
        }
        // HBMMENU_CALLBACK: the extension draws it.
        if info.hbmpItem.0 as isize == -1
            // SAFETY: the callback item belongs to the live menu retained by this walker.
            && let Some(icon) = found(unsafe { self.draw(hmenu, info) }, IconFrom::Callback)
        {
            return Some(icon);
        }
        let checkmark = bitmap(info.hbmpUnchecked).or_else(|| bitmap(info.hbmpChecked));
        if let Some(icon) = found(checkmark, IconFrom::Checkmark) {
            return Some(icon);
        }
        let program = key
            .and_then(|k| reg_string(&format!(r"{k}\command"), ""))
            .and_then(|command| program_of(&command));
        found(
            program.and_then(|exe| file_icon_image(std::path::Path::new(&exe), size)),
            IconFrom::Program,
        )
    }

    /// An `HBMMENU_CALLBACK` icon: the extension measures and draws it (`WM_MEASUREITEM`,
    /// `WM_DRAWITEM`) into a memory DC, once on black and once on white, which gives its alpha.
    unsafe fn draw(&self, hmenu: HMENU, info: &MENUITEMINFOW) -> Option<Image> {
        let send = |msg, lparam: LPARAM| -> bool {
            // SAFETY: the documented forwarding of owner-draw messages; `lparam` points at a
            // live struct for the call's duration.
            unsafe {
                match (&self.menu3, &self.menu2) {
                    (Some(m3), _) => m3.HandleMenuMsg2(msg, WPARAM(0), lparam, None).is_ok(),
                    (None, Some(m2)) => m2.HandleMenuMsg(msg, WPARAM(0), lparam).is_ok(),
                    _ => false,
                }
            }
        };
        let mut measure = MeasureItemStruct {
            CtlType: ODT_MENU,
            itemID: info.wID,
            itemData: info.dwItemData,
            ..Default::default()
        };
        send(WM_MEASUREITEM, LPARAM(std::ptr::from_mut(&mut measure) as isize));
        let fits = |v: u32| (1..=256).contains(&v);
        let (w, h) = if fits(measure.itemWidth) && fits(measure.itemHeight) {
            (measure.itemWidth as i32, measure.itemHeight as i32)
        } else {
            (self.icon, self.icon)
        };
        let paint = |background: u8| -> Option<Vec<u8>> {
            // SAFETY: a memory DC and DIB section of our own, freed before returning.
            unsafe {
                let dc = CreateCompatibleDC(None);
                let header = BITMAPINFO {
                    bmiHeader: dib_header(w, h),
                    ..Default::default()
                };
                let mut bits = std::ptr::null_mut();
                let dib = CreateDIBSection(Some(dc), &header, DIB_RGB_COLORS, &mut bits, None, 0)
                    .ok()?;
                let old = SelectObject(dc, HGDIOBJ(dib.0));
                let len = (w * h * 4) as usize;
                std::ptr::write_bytes(bits.cast::<u8>(), background, len);
                let draw = DrawItemStruct {
                    CtlType: ODT_MENU,
                    itemID: info.wID,
                    itemAction: ODA_DRAWENTIRE,
                    hwndItem: HWND(hmenu.0),
                    hDC: dc,
                    rcItem: RECT {
                        left: 0,
                        top: 0,
                        right: w,
                        bottom: h,
                    },
                    itemData: info.dwItemData,
                    ..Default::default()
                };
                let drawn = send(WM_DRAWITEM, LPARAM(std::ptr::from_ref(&draw) as isize));
                let _ = GdiFlush();
                let pixels = std::slice::from_raw_parts(bits.cast::<u8>(), len).to_vec();
                SelectObject(dc, old);
                let _ = DeleteObject(HGDIOBJ(dib.0));
                let _ = DeleteDC(dc);
                drawn.then_some(pixels)
            }
        };
        let pixels = from_black_and_white(&paint(0)?, &paint(255)?)?;
        Some(Image {
            width: w as u32,
            height: h as u32,
            pixels,
        })
    }
}

/// Premultiplied BGRA from the same drawing on black and on white: where the white shows
/// through, it is transparent. `None` if nothing was drawn.
fn from_black_and_white(black: &[u8], white: &[u8]) -> Option<Vec<u8>> {
    let mut out = vec![0u8; black.len()];
    let mut any = false;
    for ((o, b), w) in out
        .as_chunks_mut::<4>().0.iter_mut()
        .zip(black.as_chunks::<4>().0.iter())
        .zip(white.as_chunks::<4>().0.iter())
    {
        let through = (0..3).map(|c| w[c].saturating_sub(b[c])).max().unwrap_or(0);
        let alpha = 255 - through;
        any |= alpha > 0;
        for c in 0..3 {
            o[c] = b[c].min(alpha);
        }
        o[3] = alpha;
    }
    any.then_some(out)
}

/// A real menu bitmap's pixels: `0..64` and `-1` are the `HBMMENU_*` system glyphs. (A real
/// handle may have its high bit set and read as negative, so it isn't `< 64`.)
fn bitmap(hbm: HBITMAP) -> Option<Image> {
    let value = hbm.0 as isize;
    if value == -1 || (0..64).contains(&value) {
        return None;
    }
    bitmap_image(hbm)
}

/// The registry key (under `HKEY_CLASSES_ROOT`) of a desktop-background verb, if it has one.
fn verb_key(verb: &str) -> Option<String> {
    if verb.is_empty() || verb.starts_with('.') || verb.contains('\\') {
        return None;
    }
    [r"Directory\Background\shell", r"DesktopBackground\Shell"]
        .iter()
        .map(|root| format!(r"{root}\{verb}"))
        .find(|key| reg_string(key, "").is_some() || reg_string(key, "Icon").is_some())
}

/// A string value of `HKEY_CLASSES_ROOT\key` (`""`: its default value), environment expanded.
fn reg_string(key: &str, value: &str) -> Option<String> {
    let mut buffer = [0u16; 1024];
    let mut bytes = (buffer.len() * 2) as u32;
    // SAFETY: a buffer of the byte length we pass.
    let status = unsafe {
        RegGetValueW(
            HKEY_CLASSES_ROOT,
            &HSTRING::from(key),
            &HSTRING::from(value),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    if status.is_err() {
        return None;
    }
    let text = &buffer[..(bytes as usize / 2)];
    let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
    let text = String::from_utf16_lossy(&text[..end]);
    (!text.trim().is_empty()).then_some(text)
}

/// The program a command line runs: `"C:\a b\x.exe" "%V"` or `C:\a b\x.exe --flag` → the exe.
fn program_of(command: &str) -> Option<String> {
    let command = command.trim();
    let exe = if let Some(rest) = command.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        let lower = command.to_ascii_lowercase();
        match lower.find(".exe") {
            Some(end) => &command[..end + 4],
            None => command.split(' ').next()?,
        }
    };
    (!exe.is_empty()).then(|| exe.to_owned())
}

/// The icon at an icon location (`"C:\x.exe,-105"`, `"C:\x.ico"`), `size` px square.
fn extract(location: &str, size: i32) -> Option<Image> {
    let location = location.trim().trim_matches('"');
    let (path, index) = match location.rsplit_once(',') {
        Some((path, index)) if index.trim().parse::<i32>().is_ok() => {
            (path.trim().trim_matches('"'), index.trim().parse().unwrap_or(0))
        }
        _ => (location, 0),
    };
    let mut icon = HICON::default();
    // SAFETY: an out pointer to a local; the icon is ours and destroyed by `icon_image`.
    let found = unsafe {
        SHDefExtractIconW(
            &HSTRING::from(path),
            index,
            0,
            Some(&mut icon),
            None,
            size as u32 & 0xFFFF,
        )
    };
    if found.is_err() || found.0 == 1 || icon.is_invalid() {
        return None;
    }
    icon_image(icon)
}

/// An icon's pixels (premultiplied); an icon with no alpha is cut out by its mask. Destroys
/// the icon.
fn icon_image(icon: HICON) -> Option<Image> {
    let mut parts = ICONINFO::default();
    // SAFETY: the icon is ours; its bitmaps are freed below.
    unsafe {
        let image = GetIconInfo(icon, &mut parts).ok().and_then(|()| {
            let mut image = bitmap_image(parts.hbmColor)?;
            if !has_alpha(parts.hbmColor)
                && let Some(mask) = bitmap_image(parts.hbmMask)
                && mask.width == image.width
                && mask.height >= image.height
            {
                for (p, m) in image.pixels.as_chunks_mut::<4>().0.iter_mut().zip(mask.pixels.as_chunks::<4>().0.iter()) {
                    if m[0] != 0 {
                        p.copy_from_slice(&[0, 0, 0, 0]);
                    }
                }
            }
            Some(image)
        });
        let _ = DeleteObject(HGDIOBJ(parts.hbmColor.0));
        let _ = DeleteObject(HGDIOBJ(parts.hbmMask.0));
        let _ = DestroyIcon(icon);
        image
    }
}

/// Does a 32-bit bitmap carry any alpha at all?
fn has_alpha(hbm: HBITMAP) -> bool {
    dib_pixels(hbm).is_some_and(|(_, _, bits, pixels)| {
        bits == 32 && pixels.as_chunks::<4>().0.iter().any(|p| p[3] != 0)
    })
}

/// "&New" → "New"; "Open in Terminal\tCtrl+T" → "Open in Terminal".
fn clean(label: &str) -> String {
    let label = label.split('\t').next().unwrap_or_default();
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            if chars.peek() == Some(&'&') {
                out.push('&');
                chars.next();
            }
            continue;
        }
        out.push(c);
    }
    out.trim().to_owned()
}

/// No leading, trailing or doubled separators.
fn tidy(items: Vec<ShellItem>) -> Vec<ShellItem> {
    let mut out: Vec<ShellItem> = Vec::with_capacity(items.len());
    for item in items {
        let separator = matches!(item, ShellItem::Separator);
        if separator && matches!(out.last(), None | Some(ShellItem::Separator)) {
            continue;
        }
        out.push(item);
    }
    while matches!(out.last(), Some(ShellItem::Separator)) {
        out.pop();
    }
    out
}

/// Runs command `id` the way Explorer does: the click point, the desktop as the directory.
fn invoke(menu: &IContextMenu, id: u32, at: (i32, i32)) -> Result<()> {
    let directory = glaze_desktop::desktop_view::desktop_folder()
        .ok()
        .map(|d| HSTRING::from(d.as_os_str()));
    let info = CMINVOKECOMMANDINFOEX {
        cbSize: size_of::<CMINVOKECOMMANDINFOEX>() as u32,
        fMask: CMIC_MASK_UNICODE | CMIC_MASK_PTINVOKE,
        hwnd: HWND::default(),
        // MAKEINTRESOURCE: the command's offset from FIRST.
        lpVerb: PCSTR((id - FIRST) as usize as *const u8),
        lpVerbW: PCWSTR((id - FIRST) as usize as *const u16),
        lpDirectoryW: directory
            .as_ref()
            .map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr())),
        nShow: SW_SHOWNORMAL.0,
        ptInvoke: POINT { x: at.0, y: at.1 },
        ..Default::default()
    };
    // SAFETY: `info` is a CMINVOKECOMMANDINFOEX whose strings outlive the call; the shell reads
    // it through the base struct's pointer.
    unsafe { menu.InvokeCommand(std::ptr::from_ref(&info).cast::<CMINVOKECOMMANDINFO>()) }?;
    Ok(())
}

/// A file's or program's own icon at `size` px (premultiplied), as the shell draws it.
pub fn file_icon(path: &std::path::Path, size: i32) -> Option<Arc<Image>> {
    file_icon_image(path, size).map(Arc::new)
}

fn file_icon_image(path: &std::path::Path, size: i32) -> Option<Image> {
    if !path.is_file() {
        return None;
    }
    // SAFETY: documented shell calls; the returned bitmap is ours and deleted after the copy.
    unsafe {
        let item: IShellItemImageFactory =
            SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None).ok()?;
        let hbm = item
            .GetImage(
                windows::Win32::Foundation::SIZE { cx: size, cy: size },
                SIIGBF_ICONONLY,
            )
            .ok()?;
        let image = bitmap_image(hbm);
        let _ = DeleteObject(HGDIOBJ(hbm.0));
        image
    }
}

/// The icon of files of type `extension` (".docx") at `size` px, as Explorer's "New" shows
/// it: the icon of the type the "New" entry registers (`HKCR\.docx` → `Word.Document.12`), else
/// the type's resolved icon, else the icon of the program that opens it, else the shell's own.
/// (The resolved association can be the generic page, e.g. a default app that is gone.)
fn type_icon(extension: &str, size: i32) -> Option<ShellIcon> {
    let found = |image: Option<Image>, from| {
        image.map(|image| ShellIcon {
            image: Arc::new(image),
            from,
        })
    };
    let registered = reg_string(extension, "")
        .and_then(|progid| reg_string(&format!(r"{progid}\DefaultIcon"), ""))
        .into_iter()
        .chain(assoc(extension, ASSOCSTR_DEFAULTICON))
        .filter(|i| !i.contains("%1"))
        .find_map(|i| extract(&i, size));
    if let Some(icon) = found(registered, IconFrom::Type) {
        return Some(icon);
    }
    let program = assoc(extension, ASSOCSTR_EXECUTABLE)
        .filter(|exe| !exe.to_ascii_lowercase().ends_with("openwith.exe"))
        .and_then(|exe| file_icon_image(std::path::Path::new(&exe), size));
    if let Some(icon) = found(program, IconFrom::Program) {
        return Some(icon);
    }
    found(shell_type_icon(extension), IconFrom::Type)
}

/// What `AssocQueryStringW` says about `extension`.
fn assoc(extension: &str, what: ASSOCSTR) -> Option<String> {
    let mut buffer = [0u16; 1024];
    let mut len = buffer.len() as u32;
    // SAFETY: a buffer of the length we pass.
    let ok = unsafe {
        AssocQueryStringW(
            ASSOCF_NONE,
            what,
            &HSTRING::from(extension),
            PCWSTR::null(),
            Some(PWSTR(buffer.as_mut_ptr())),
            &mut len,
        )
    };
    if ok.is_err() {
        return None;
    }
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    let text = String::from_utf16_lossy(&buffer[..end]);
    (!text.trim().is_empty()).then_some(text)
}

/// The shell's small icon for files of type `extension`.
fn shell_type_icon(extension: &str) -> Option<Image> {
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
    use windows::Win32::UI::Shell::{
        SHFILEINFOW, SHGFI_ICON, SHGFI_SMALLICON, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
    };
    let mut info = SHFILEINFOW::default();
    // SAFETY: a valid out struct of the size we pass; the icon is freed by `icon_image`.
    unsafe {
        let found = SHGetFileInfoW(
            &HSTRING::from(extension),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_SMALLICON | SHGFI_USEFILEATTRIBUTES,
        );
        if found == 0 || info.hIcon.is_invalid() {
            return None;
        }
    }
    icon_image(info.hIcon)
}

fn dib_header(w: i32, h: i32) -> BITMAPINFOHEADER {
    BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        biHeight: -h,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    }
}

/// A bitmap's pixels as top-down 32-bit BGRA, as stored: (width, height, its own bit depth,
/// pixels).
fn dib_pixels(hbm: HBITMAP) -> Option<(i32, i32, u16, Vec<u8>)> {
    // SAFETY: plain GDI queries into buffers of the sizes we pass.
    unsafe {
        let mut bm = BITMAP::default();
        if GetObjectW(
            HGDIOBJ(hbm.0),
            size_of::<BITMAP>() as i32,
            Some(std::ptr::from_mut(&mut bm).cast()),
        ) == 0
        {
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
        if w <= 0 || h <= 0 || w > 256 || h > 512 {
            return None;
        }
        let mut info = BITMAPINFO {
            bmiHeader: dib_header(w, h),
            ..Default::default()
        };
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        let dc = GetDC(None);
        let lines = GetDIBits(
            dc,
            hbm,
            0,
            h as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, dc);
        if lines == 0 {
            return None;
        }
        Some((w, h, bm.bmBitsPixel, pixels))
    }
}

/// A bitmap's pixels, top-down premultiplied BGRA. A bitmap with no alpha at all is taken as
/// opaque; straight alpha is premultiplied.
fn bitmap_image(hbm: HBITMAP) -> Option<Image> {
    let (w, h, bits, mut pixels) = dib_pixels(hbm)?;
    premultiply(&mut pixels, bits == 32);
    Some(Image {
        width: w as u32,
        height: h as u32,
        pixels,
    })
}

/// Makes BGRA `pixels` premultiplied: no alpha at all (or a bitmap that has none) means
/// opaque; any colour channel above its alpha means straight alpha.
fn premultiply(pixels: &mut [u8], has_alpha: bool) {
    let any_alpha = has_alpha && pixels.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
    if !any_alpha {
        for p in pixels.as_chunks_mut::<4>().0.iter_mut() {
            p[3] = 255;
        }
        return;
    }
    let straight = pixels
        .as_chunks::<4>().0.iter()
        .any(|p| p[0] > p[3] || p[1] > p[3] || p[2] > p[3]);
    if straight {
        for p in pixels.as_chunks_mut::<4>().0.iter_mut() {
            let a = u16::from(p[3]);
            for c in &mut p[..3] {
                *c = ((u16::from(*c) * a + 127) / 255) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_lose_their_access_keys_and_shortcuts() {
        assert_eq!(clean("&New"), "New");
        assert_eq!(clean("Open in &Terminal\tCtrl+T"), "Open in Terminal");
        assert_eq!(clean("Fish && Chips"), "Fish & Chips");
    }

    #[test]
    fn verbs_run_the_program_their_command_names() {
        let exe = |c| program_of(c);
        assert_eq!(
            exe(r#""C:\Program Files\Git\git-bash.exe" "--cd=%v.""#).as_deref(),
            Some(r"C:\Program Files\Git\git-bash.exe")
        );
        assert_eq!(
            exe(r#"C:\Program Files\Alacritty\alacritty.exe --working-directory "%v""#).as_deref(),
            Some(r"C:\Program Files\Alacritty\alacritty.exe")
        );
    }

    #[test]
    fn a_drawing_on_black_and_on_white_gives_its_alpha() {
        // Opaque red, half-transparent white, untouched background.
        let black = [0, 0, 255, 0, 128, 128, 128, 0, 0, 0, 0, 0];
        let white = [0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255];
        assert_eq!(
            from_black_and_white(&black, &white).unwrap(),
            [0, 0, 255, 255, 128, 128, 128, 128, 0, 0, 0, 0]
        );
        assert_eq!(from_black_and_white(&[0; 4], &[255; 4]), None);
    }

    #[test]
    fn bitmaps_come_out_premultiplied() {
        // Straight alpha: half-transparent white.
        let mut straight = vec![255, 255, 255, 128];
        premultiply(&mut straight, true);
        assert_eq!(straight, [128, 128, 128, 128]);
        // Already premultiplied: left alone.
        let mut premul = vec![100, 100, 100, 128];
        premultiply(&mut premul, true);
        assert_eq!(premul, [100, 100, 100, 128]);
        // No alpha channel at all: opaque.
        let mut opaque = vec![10, 20, 30, 0];
        premultiply(&mut opaque, true);
        assert_eq!(opaque, [10, 20, 30, 255]);
    }
}

#[cfg(test)]
mod probe {
    /// `cargo test -p glaze-shell shell_menu::probe -- --ignored --nocapture`: prints this
    /// machine's desktop menu as the worker sees it.
    #[test]
    #[ignore = "reads this machine's registered desktop shell extensions"]
    fn print_the_desktop_menu() {
        let menu = super::ShellMenu::start(16).unwrap();
        let started = std::time::Instant::now();
        while menu.snapshot().is_none() && started.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let snapshot = menu.snapshot().expect("a snapshot");
        println!("built in {:?}", snapshot.took);
        fn print(items: &[super::ShellItem], depth: usize) {
            for item in items {
                let pad = "  ".repeat(depth);
                match item {
                    super::ShellItem::Separator => println!("{pad}---"),
                    super::ShellItem::Action {
                        label,
                        verb,
                        id,
                        icon,
                        enabled,
                    } => println!(
                        "{pad}{label} [{verb}] #{id} icon={:?} enabled={enabled}",
                        icon.as_ref().map(|i| (i.from, i.image.width, i.image.height))
                    ),
                    super::ShellItem::Submenu { label, icon, items } => {
                        println!(
                            "{pad}{label} > icon={:?}",
                            icon.as_ref().map(|i| (i.from, i.image.width, i.image.height))
                        );
                        print(items, depth + 1);
                    }
                }
            }
        }
        print(&snapshot.items, 0);
    }
}

#[cfg(test)]
mod live {
    use super::super::items::{self, Sources};
    use glaze_ui::{Entry, Tokens, Ui, icons, menu};

    /// `cargo test -p glaze-shell shell_menu::live -- --ignored --nocapture`: draws this
    /// machine's full desktop menu (its shell extensions, their icons, the desktop's state) and
    /// its View, Sort by, New and Open in submenus to `%TEMP%\glaze-desktop-menu-*.png`, as
    /// the shell draws them (at `GLAZE_MENU_SCALE`, default 1), and times building it.
    #[test]
    #[ignore = "renders this machine's desktop shell extensions; writes capture images"]
    fn draw_this_machines_desktop_menu() {
        let scale: f32 = std::env::var("GLAZE_MENU_SCALE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.0);
        let shell = super::ShellMenu::start((16.0 * scale).round() as i32).unwrap();
        let started = std::time::Instant::now();
        while shell.snapshot().is_none() && started.elapsed().as_secs() < 10 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let snapshot = shell.snapshot().expect("a snapshot");
        let state = super::super::view_state();
        let mut ui = Ui::new().unwrap();
        let theme = glaze_ui::Theme::new(
            glaze_ui::Style::Liquid,
            glaze_ui::Scheme::Dark,
            glaze_ui::theme::SYSTEM_BLUE,
        );
        let tokens = Tokens {
            icons: icons::system_family(&ui.text),
            glass: None,
            ..theme.tokens()
        };
        let built = std::time::Instant::now();
        // The installed plugins' items (`desktop-menu`), as the shell adds them.
        let plugins = crate::widgets::plugin_events::menu_items();
        let root = items::build_with(
            &glaze_config::DesktopMenu::default(),
            state,
            Sources {
                plugins: &plugins,
                shell: Some(&snapshot),
                Glaze: None,
            },
        );
        let sub = |label: &str| {
            root.iter()
                .find(|i| i.label() == label)
                .and_then(|i| match i.plain() {
                    Entry::Submenu { items, .. } => Some(items.clone()),
                    _ => None,
                })
                .unwrap()
        };
        let mut panels = vec![
            ("root", root.clone(), Some(2)),
            ("view", sub("View"), None),
            ("sort", sub("Sort by"), None),
            ("new", sub("New"), None),
            ("open", sub("Open in"), None),
        ];
        // Each plugin's submenu, with the root hovering the first of them.
        for plugin in plugins.iter().map(|c| c.name.clone()).collect::<std::collections::BTreeSet<_>>() {
            let at = root.iter().position(|i| i.label() == plugin);
            panels[0].2 = panels[0].2.max(at);
            panels.push(("plugin", sub(&plugin), None));
        }
        for (name, items, hover) in panels {
            let layout = menu::layout(&items, &tokens, scale, &mut ui.text);
            let list = menu::view(&items, &layout, hover, &tokens);
            let edge = tokens.menu.shadow.ceil() as u32;
            let (w, h) = layout.size();
            let image =
                glaze_ui::render(&mut ui, &list, w as u32 + 2 * edge, h as u32 + 2 * edge)
                    .unwrap();
            let path = std::env::temp_dir().join(format!("glaze-desktop-menu-{name}.png"));
            glaze_ui::golden::write(&image, &path).unwrap();
            println!("{}", path.display());
        }
        println!(
            "shell items built (worker, off the UI thread) in {:?}; items + layout + 5 panels drawn in {:?}",
            snapshot.took,
            built.elapsed()
        );
    }
}
