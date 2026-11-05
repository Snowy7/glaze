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
use mypaper_ui::Image;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HBITMAP, HGDIOBJ, ReleaseDC,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::Shell::{
    CMF_NORMAL, CMIC_MASK_PTINVOKE, CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX, GCS_VERBW,
    IContextMenu, IContextMenu2, IContextMenu3, IShellFolder, IShellItemImageFactory,
    SHCreateItemFromParsingName, SHGetDesktopFolder, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetDesktopWindow, GetMenuItemCount, GetMenuItemInfoW, HMENU,
    MENUITEMINFOW, MFS_DISABLED, MFT_OWNERDRAW, MFT_SEPARATOR, MIIM_BITMAP, MIIM_FTYPE, MIIM_ID,
    MIIM_STATE, MIIM_STRING, MIIM_SUBMENU, SW_SHOWNORMAL, WM_INITMENUPOPUP,
};
use windows::core::{HSTRING, Interface, PCSTR, PCWSTR, PWSTR};

/// The command ids handed to `QueryContextMenu`: `FIRST..=LAST`.
const FIRST: u32 = 1;
const LAST: u32 = 0x7FFF;
/// A worker busy for longer than this (an extension hung) is given up on.
const HANG: Duration = Duration::from_millis(1500);
/// `CMIC_MASK_UNICODE` (`SEE_MASK_UNICODE`): `lpVerbW` and `lpDirectoryW` are read.
const CMIC_MASK_UNICODE: u32 = 0x4000;

/// One item of the shell's menu, flattened.
#[derive(Debug, Clone)]
pub enum ShellItem {
    Separator,
    Action {
        label: String,
        /// The canonical verb, when the extension names one ("paste", "undo", "git_shell").
        verb: String,
        id: u32,
        icon: Option<Arc<Image>>,
        enabled: bool,
    },
    Submenu {
        label: String,
        icon: Option<Arc<Image>>,
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
                fMask: MIIM_FTYPE | MIIM_ID | MIIM_STRING | MIIM_SUBMENU | MIIM_STATE | MIIM_BITMAP,
                dwTypeData: PWSTR(text.as_mut_ptr()),
                cch: text.len() as u32 - 1,
                ..Default::default()
            };
            if unsafe { GetMenuItemInfoW(hmenu, index, true, &mut info) }.is_err() {
                continue;
            }
            if info.fType.0 & MFT_SEPARATOR.0 != 0 {
                out.push(ShellItem::Separator);
                continue;
            }
            let label = String::from_utf16_lossy(&text[..info.cch as usize]);
            let label = clean(&label);
            let mut icon = self.bitmap(info.hbmpItem);
            if !info.hSubMenu.is_invalid() && depth < 3 {
                self.init_popup(info.hSubMenu, index);
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
            // "New" types without a bitmap: the type's own icon (the verb is its extension).
            if icon.is_none() && verb.starts_with('.') {
                icon = type_icon(&verb, self.icon);
            }
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

    fn bitmap(&self, hbm: HBITMAP) -> Option<Arc<Image>> {
        // Real bitmaps only: small values are the HBMMENU_* system glyphs.
        if (hbm.0 as isize) < 64 {
            return None;
        }
        bitmap_image(hbm).map(Arc::new)
    }
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
    let directory = mypaper_desktop::desktop_view::desktop_folder()
        .ok()
        .map(|d| HSTRING::from(d.as_os_str()));
    let info = CMINVOKECOMMANDINFOEX {
        cbSize: size_of::<CMINVOKECOMMANDINFOEX>() as u32,
        fMask: CMIC_MASK_UNICODE | CMIC_MASK_PTINVOKE as u32,
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
        image.map(Arc::new)
    }
}

/// The small icon of files of type `extension` (".docx"), as Explorer shows them.
fn type_icon(extension: &str, _size: i32) -> Option<Arc<Image>> {
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
    use windows::Win32::UI::Shell::{
        SHFILEINFOW, SHGFI_ICON, SHGFI_SMALLICON, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};
    let mut info = SHFILEINFOW::default();
    // SAFETY: a valid out struct of the size we pass; the icon and its bitmaps are ours to free.
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
        let mut parts = ICONINFO::default();
        let image = GetIconInfo(info.hIcon, &mut parts)
            .ok()
            .and_then(|()| bitmap_image(parts.hbmColor));
        let _ = DeleteObject(HGDIOBJ(parts.hbmColor.0));
        let _ = DeleteObject(HGDIOBJ(parts.hbmMask.0));
        let _ = DestroyIcon(info.hIcon);
        image.map(Arc::new)
    }
}

/// A 32-bit bitmap's pixels, top-down premultiplied BGRA. A bitmap with no alpha at all is
/// taken as opaque; straight alpha is premultiplied.
fn bitmap_image(hbm: HBITMAP) -> Option<Image> {
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
        if w <= 0 || h <= 0 || w > 256 || h > 256 {
            return None;
        }
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
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
        premultiply(&mut pixels, bm.bmBitsPixel == 32);
        Some(Image {
            width: w as u32,
            height: h as u32,
            pixels,
        })
    }
}

/// Makes BGRA `pixels` premultiplied: no alpha at all (or a bitmap that has none) means
/// opaque; any colour channel above its alpha means straight alpha.
fn premultiply(pixels: &mut [u8], has_alpha: bool) {
    let any_alpha = has_alpha && pixels.chunks_exact(4).any(|p| p[3] != 0);
    if !any_alpha {
        for p in pixels.chunks_exact_mut(4) {
            p[3] = 255;
        }
        return;
    }
    let straight = pixels
        .chunks_exact(4)
        .any(|p| p[0] > p[3] || p[1] > p[3] || p[2] > p[3]);
    if straight {
        for p in pixels.chunks_exact_mut(4) {
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
    /// `cargo test -p mypaper-shell shell_menu::probe -- --ignored --nocapture`: prints this
    /// machine's desktop menu as the worker sees it.
    #[test]
    #[ignore]
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
                        icon.as_ref().map(|i| (i.width, i.height))
                    ),
                    super::ShellItem::Submenu { label, icon, items } => {
                        println!(
                            "{pad}{label} > icon={:?}",
                            icon.as_ref().map(|i| (i.width, i.height))
                        );
                        print(items, depth + 1);
                    }
                }
            }
        }
        print(&snapshot.items, 0);
    }
}
